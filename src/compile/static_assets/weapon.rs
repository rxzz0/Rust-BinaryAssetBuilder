use super::binary::BinaryData;
use super::tokenized::{self, Entry, Table, Target};
use super::{
    Marshaled, asset_reference, finite_number, sage_time, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::{io, path::Path, sync::OnceLock};

// Read the fixed token table and default fields from the empty EA fixture.
const BASE: &[u8] = include_bytes!(
    "../../../tests/fixtures/ea-batches/weapon-template-root-assets/94d4d96e.0ce6168e.a529f9d4.226acfbb.asset"
);

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ROOT: usize = 0x514;
    const ATTRS: [(&str, usize, &str); 3] = [
        ("AttackRange", 0, "0"),
        ("MinimumAttackRange", 4, "0"),
        ("WeaponSpeed", 0x28, "99999"),
    ];
    validate_attributes(
        node,
        &[
            "id",
            "AttackRange",
            "MinimumAttackRange",
            "WeaponSpeed",
            "ClipSize",
            "CanFireWhileMoving",
            "Flags",
            "RequiredAntiMask",
            "RadiusDamageAffects",
        ],
        path,
    )?;
    let mut data = BinaryData {
        payload: BASE[32 + ROOT..].to_vec(),
        relocations: Vec::new(),
        path,
    };
    let mut table = Table::from_layout(&BASE[32..32 + ROOT], &data.payload, path)?;
    for (name, offset, default) in ATTRS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    let clip = node
        .attribute("ClipSize")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid WeaponTemplate ClipSize"))?;
    data.word(0x6c, clip as u32);
    data.payload[0x126] = match node
        .attribute("CanFireWhileMoving")
        .unwrap_or("false")
        .trim()
    {
        "true" | "1" => 1,
        "false" | "0" => 0,
        _ => return Err(invalid(path, "invalid CanFireWhileMoving")),
    };
    for (name, kind, offset) in [
        ("Flags", 0, 0xa8),
        ("RadiusDamageAffects", 1, 0xc0),
        ("RequiredAntiMask", 2, 0xcc),
    ] {
        if let Some(value) = node.attribute(name) {
            data.word(offset, weapon_flags(value, kind, path)?);
        }
    }
    let mut children = significant_children(node).peekable();
    for (name, offset) in [
        ("PreAttackDelay", 0xe8),
        ("FiringDuration", 0xec),
        ("CoolDownDelayBetweenShots", 0xf0),
        ("ClipReloadTime", 0xf4),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            validate_attributes(child, &["MinSeconds", "MaxSeconds"], path)?;
            if has_content(child) {
                return Err(invalid(path, format!("unexpected {name} content")));
            }
            let start = data.allocate(offset, 8)?;
            let mut entries = Vec::new();
            for (field, offset) in [("MinSeconds", start), ("MaxSeconds", start + 4)] {
                data.word(
                    offset,
                    sage_time(child.attribute(field).unwrap_or("0s"), field, path)?.to_bits(),
                );
                entries.push(Entry::named(field, 0x9986_69e8, Target::Data(offset)));
            }
            if !table.replace_target(name, Target::Table(Table::record(entries))) {
                return Err(invalid(path, format!("weapon layout missing {name}")));
            }
        }
    }
    let mut out = Marshaled::default();
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Nuggets"))) {
        let nuggets = encode_nuggets(child, &mut data, &mut out)?;
        if !table.replace_target("Nuggets", Target::Table(nuggets)) {
            return Err(invalid(path, "weapon layout missing Nuggets"));
        }
    }
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!(
                "unsupported WeaponTemplate child {}",
                child.tag_name().name()
            ),
        ));
    }
    data.relocations.sort_unstable();
    tokenized::finish(table, data, out)
}

fn encode_nuggets(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    validate_attributes(node, &[], data.path)?;
    let children: Vec<_> = significant_children(node).collect();
    let first = data.list(0xfc, children.len(), 4)?;
    let mut entries = Vec::with_capacity(children.len());
    for (index, child) in children.into_iter().enumerate() {
        let pointer = first + index * 4;
        let (kind, target) = match (child.tag_name().namespace(), child.tag_name().name()) {
            (Some(SAGE_NS), "FireOnObjectsNugget") => {
                (0x7814_daf0, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "TintObjectsNugget") => {
                (0xda74_d739, tint_objects(child, pointer, data)?)
            }
            (Some(SAGE_NS), "DOTNugget") => {
                (0xefbb_5226, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "DamageNugget") => {
                (0xc0f5_3e99, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "ProjectileNugget") => (
                0x87c7_bdcc,
                simple_reference_nugget(child, pointer, data, out, true)?,
            ),
            (Some(SAGE_NS), "WeaponOCLNugget") => (
                0xc606_7c3f,
                simple_reference_nugget(child, pointer, data, out, false)?,
            ),
            _ => {
                return Err(invalid(
                    data.path,
                    format!("unsupported weapon nugget {}", child.tag_name().name()),
                ));
            }
        };
        entries.push((
            0,
            Target::Table(Table::sequence(vec![Entry::new(
                2,
                kind,
                Target::Table(target),
            )])),
        ));
    }
    Ok(Table::array(0xfc, entries))
}

fn tint_objects(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    const FLOAT: u32 = 0x6458_64f3;
    const TIME: u32 = 0x9986_69e8;
    let path = data.path;
    validate_attributes(
        node,
        &[
            "Radius",
            "PreColorTime",
            "PostColorTime",
            "SustainedColorTime",
            "Frequency",
            "Amplitude",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x48)?;
    data.word(start, 0xda74_d739);
    data.word(start + 4, 4);
    data.word(
        start + 0x10,
        finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?.to_bits(),
    );
    let mut entries = vec![Entry::new(
        1,
        0xc7c2_cc04,
        Target::Table(effect_table(start)),
    )];
    for (index, (name, kind, default)) in [
        ("PreColorTime", TIME, "2s"),
        ("PostColorTime", TIME, "2s"),
        ("SustainedColorTime", TIME, "1s"),
        ("Frequency", FLOAT, "1"),
        ("Amplitude", FLOAT, "1"),
    ]
    .into_iter()
    .enumerate()
    {
        let value = node.attribute(name).unwrap_or(default);
        let value = if kind == TIME {
            sage_time(value, name, path)?
        } else {
            finite_number(value, name, path)?
        };
        let offset = start + 0x28 + index * 4;
        data.word(offset, value.to_bits());
        entries.push(Entry::named(name, kind, Target::Data(offset)));
    }
    let mut children = significant_children(node);
    let color = children.next();
    if color.is_some_and(|child| !child.has_tag_name((SAGE_NS, "Color")) || has_content(child))
        || children.next().is_some()
    {
        return Err(invalid(path, "unexpected TintObjectsNugget child"));
    }
    let mut rgb = Vec::new();
    for (index, name) in ["R", "G", "B"].into_iter().enumerate() {
        let offset = start + 0x3c + index * 4;
        if let Some(color) = color {
            validate_attributes(color, &["R", "G", "B"], path)?;
            data.word(
                offset,
                finite_number(required(color, name, path)?, name, path)?.to_bits(),
            );
        }
        rgb.push(Entry::named(name, FLOAT, Target::Data(offset)));
    }
    entries.push(Entry::named(
        "Color",
        0xaee7_62a5,
        Target::Table(Table::record(rgb)),
    ));
    Ok(Table::record(entries))
}

fn fire_on_objects(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    const DAMAGE_FIELDS: &[(u32, u32, usize)] = &[
        (0x7c33e4e9, 0x645864f3, 0x28),
        (0x1182ea06, 0x645864f3, 0x2c),
        (0xec392b69, 0x645864f3, 0x30),
        (0x1ca750fc, 0x4e16ea4d, 0x34),
        (0x0cfd3b28, 0x77e3f4f7, 0x90),
        (0x06298750, 0x645864f3, 0x38),
        (0x229b6ad7, 0x645864f3, 0x3c),
        (0x792ea9b5, 0x77e3f4f7, 0x91),
        (0x827ad6d4, 0x645864f3, 0x40),
        (0x6ba7b51a, 0x645864f3, 0x44),
        (0x8ac7e66a, 0x998669e8, 0x48),
        (0x46486ff1, 0x46486ff1, 0x4c),
        (0xc608ed36, 0xc608ed36, 0x50),
        (0x2faa4e62, 0x2faa4e62, 0x54),
        (0x62bb1ce7, 0x62bb1ce7, 0x58),
        (0x6fbb66ba, 0x77e3f4f7, 0x92),
        (0xf37c08fc, 0x645864f3, 0x5c),
        (0x722223ed, 0x77e3f4f7, 0x93),
        (0x31a847b4, 0x645864f3, 0x60),
        (0xfa8dab3f, 0xc4ec43b1, 0x64),
        (0x2dfa364c, 0xbfab9075, 0x68),
        (0x53fc2b18, 0x77e3f4f7, 0x94),
        (0x063c4040, 0x77e3f4f7, 0x95),
        (0x45c3978c, 0x77e3f4f7, 0x96),
        (0x21494542, 0x77e3f4f7, 0x97),
        (0xde8679a4, 0xa7bd811e, 0x6c),
    ];
    let path = data.path;
    let dot = node.has_tag_name((SAGE_NS, "DOTNugget"));
    let direct = node.has_tag_name((SAGE_NS, "DamageNugget"));
    let is_damage = dot || direct;
    validate_attributes(
        node,
        if is_damage {
            &[
                "id",
                "Radius",
                "PartitionFilterTestType",
                "Damage",
                "DamageInterval",
                "DamageDuration",
                "DamageType",
                "DamageFXType",
                "DeathType",
                "RemoveIfHealed",
            ]
        } else {
            &[
                "Radius",
                "PartitionFilterTestType",
                "Weapon",
                "FireOnVictimObject",
                "VictimMustBeAlive",
            ]
        },
        path,
    )?;
    let start = data.allocate(pointer, if direct { 0x98 } else { 0xa4 })?;
    for (offset, value) in [
        (0, 0x7814_daf0),
        (0x2c, (-1f32).to_bits()),
        (0x34, std::f32::consts::TAU.to_bits()),
        (0x38, (-1f32).to_bits()),
        (0x3c, (-1f32).to_bits()),
        (0x44, 1.0f32.to_bits()),
        (0x4c, 14),
        (0x50, 1),
        (0x54, 112),
        (0x5c, 1.0f32.to_bits()),
        (0x70, 0xa000_0000),
        (0x84, 0x0001_0000),
        (0x90, 0x100),
    ] {
        data.word(start + offset, value);
    }
    let partition = super::global_ui::enum_value(
        node.attribute("PartitionFilterTestType")
            .unwrap_or("EXTENTS_3D"),
        &["CENTER_2D", "CIRCLE", "CENTER_3D", "SPHERE", "EXTENTS_3D"],
        path,
    )?;
    data.word(start + 4, partition);
    data.word(
        start + 0x10,
        finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?.to_bits(),
    );
    if is_damage {
        data.word(start, if dot { 0xefbb_5226 } else { 0xc0f5_3e99 });
        data.word(
            start + 0x28,
            finite_number(node.attribute("Damage").unwrap_or("0"), "Damage", path)?.to_bits(),
        );
        for (index, (name, default, schema)) in [
            (
                "DamageType",
                "UNDEFINED",
                include_str!("../../../schemas/ra3/xsd/Includes/DamageDef.xsd"),
            ),
            (
                "DeathType",
                "NORMAL",
                include_str!("../../../schemas/ra3/xsd/Includes/Death.xsd"),
            ),
            (
                "DamageFXType",
                "UNDEFINED",
                include_str!("../../../schemas/ra3/xsd/Includes/DamageFX.xsd"),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            static VALUES: [OnceLock<Vec<String>>; 3] = [const { OnceLock::new() }; 3];
            let values = VALUES[index].get_or_init(|| schema_enums(schema, &[name])[0].clone());
            let value = node.attribute(name).unwrap_or(default);
            let ordinal = values
                .iter()
                .position(|choice| choice == value)
                .ok_or_else(|| invalid(path, format!("invalid {name} {value}")))?
                as u32;
            data.word(
                start + 0x4c + index * 4,
                if index == 2 {
                    ordinal.wrapping_sub(1)
                } else {
                    ordinal
                },
            );
        }
    }
    for (name, offset, default) in [
        ("FireOnVictimObject", 0xa0, "false"),
        ("VictimMustBeAlive", 0xa1, "true"),
    ] {
        if is_damage {
            break;
        }
        data.payload[start + offset] = match node.attribute(name).unwrap_or(default).trim() {
            "true" | "1" => 1,
            "false" | "0" => 0,
            _ => return Err(invalid(path, format!("invalid {name}"))),
        };
    }
    if let Some(value) = node
        .attribute("Weapon")
        .filter(|value| !value.trim().is_empty())
    {
        let id = asset_reference(
            value.split_once('\\').map_or(value, |(name, _)| name),
            "WeaponTemplate",
            path,
        )?;
        out.reference_types.push(id.type_id);
        out.references.push(id.into());
        data.word(start + 0x98, out.references.len() as u32);
    }
    let mut children = significant_children(node);
    let filter = if let Some(filter) = children.next() {
        let name = if is_damage {
            "SpecialObjectFilter"
        } else {
            "Filter"
        };
        if !filter.has_tag_name((SAGE_NS, name)) || children.next().is_some() {
            return Err(invalid(path, "unsupported FireOnObjectsNugget child"));
        }
        Target::Table(super::game_object::encode_object_filter(
            filter,
            start + if is_damage { 0x14 } else { 0x9c },
            data,
            &mut out.reference_types,
        )?)
    } else {
        Target::Null
    };
    let mut effect = effect_table(start);
    let filter = if is_damage {
        effect.replace_target("SpecialObjectFilter", filter);
        Target::Null
    } else {
        filter
    };
    let mut damage = vec![Entry::new(1, 0xc7c2_cc04, Target::Table(effect))];
    damage.extend(
        DAMAGE_FIELDS
            .iter()
            .map(|&(name, kind, offset)| Entry::new(name, kind, Target::Data(start + offset))),
    );
    damage.push(Entry::new(
        0x9197_20cd,
        0xe4a6_e6e4,
        Target::Table(Table::array(start + 0x88, [])),
    ));
    if direct {
        return Ok(Table::record(damage));
    }
    if dot {
        let mut entries = vec![Entry::new(
            1,
            0xc0f5_3e99,
            Target::Table(Table::record(damage)),
        )];
        for (name, offset) in [("DamageInterval", 0x98), ("DamageDuration", 0x9c)] {
            data.word(
                start + offset,
                sage_time(required(node, name, path)?, name, path)?.to_bits(),
            );
            entries.push(Entry::named(
                name,
                0x9986_69e8,
                Target::Data(start + offset),
            ));
        }
        data.payload[start + 0xa0] = flag(node, "RemoveIfHealed", true, path)?;
        entries.push(Entry::named(
            "RemoveIfHealed",
            0x77e3_f4f7,
            Target::Data(start + 0xa0),
        ));
        return Ok(Table::record(entries));
    }
    Ok(Table::record(vec![
        Entry::new(1, 0xc0f5_3e99, Target::Table(Table::record(damage))),
        Entry::named("Weapon", 0xd046_dd22, Target::Data(start + 0x98)),
        Entry::named(
            "FireOnVictimObject",
            0x77e3_f4f7,
            Target::Data(start + 0xa0),
        ),
        Entry::named("VictimMustBeAlive", 0x77e3_f4f7, Target::Data(start + 0xa1)),
        Entry::named("Filter", 0xe46e_a3b2, filter),
    ]))
}

fn flag(node: Node<'_, '_>, name: &str, default: bool, path: &Path) -> io::Result<u8> {
    match node.attribute(name).map(str::trim) {
        None => Ok(u8::from(default)),
        Some("true" | "1") => Ok(1),
        Some("false" | "0") => Ok(0),
        _ => Err(invalid(path, format!("invalid {name}"))),
    }
}

fn simple_reference_nugget(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
    projectile: bool,
) -> io::Result<Table> {
    let path = data.path;
    let refs: &[(&str, u32, usize, &str)] = if projectile {
        &[
            ("WarheadTemplate", 0xd046_dd22, 0x28, "WeaponTemplate"),
            ("ProjectileTemplate", 0xbfab_9075, 0x2c, "GameObject"),
        ]
    } else {
        &[("WeaponOCL", 0x42e4_e9d1, 0x28, "ObjectCreationList")]
    };
    let allowed: Vec<_> = refs
        .iter()
        .map(|&(name, ..)| name)
        .chain((!projectile).then_some("TargetAsPrimaryObject"))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected weapon nugget child"));
    }
    let start = data.allocate(pointer, if projectile { 0x44 } else { 0x30 })?;
    data.word(start, if projectile { 0x87c7_bdcc } else { 0xc606_7c3f });
    data.word(start + 4, 4);
    for attr in node.attributes() {
        if let Some(&(_, _, offset, kind)) = refs.iter().find(|&&(name, ..)| name == attr.name())
            && !attr.value().trim().is_empty()
        {
            let value = attr
                .value()
                .split_once('\\')
                .map_or(attr.value(), |(name, _)| name);
            let id = asset_reference(value, kind, path)?;
            out.reference_types.push(id.type_id);
            out.references.push(id.into());
            data.word(start + offset, out.references.len() as u32);
        }
    }
    let mut entries = vec![Entry::new(
        1,
        0xc7c2_cc04,
        Target::Table(effect_table(start)),
    )];
    entries
        .extend(refs.iter().map(|&(name, kind, offset, _)| {
            Entry::named(name, kind, Target::Data(start + offset))
        }));
    if projectile {
        data.word(start + 0x30, 5);
        entries.extend([
            Entry::named(
                "WeaponLaunchBoneSlotOverride",
                0x1848_c549,
                Target::Data(start + 0x30),
            ),
            Entry::named("AttackOffset", 0x2936_8077, Target::Null),
            Entry::named(
                "VeterancyProjectiles",
                0x4185_7548,
                Target::Table(Table::array(start + 0x38, [])),
            ),
            Entry::named("SpecificBarrelOverride", 0x23e6_50b8, Target::Null),
        ]);
    } else {
        data.payload[start + 0x2c] = flag(node, "TargetAsPrimaryObject", false, path)?;
        entries.push(Entry::named(
            "TargetAsPrimaryObject",
            0x77e3_f4f7,
            Target::Data(start + 0x2c),
        ));
    }
    Ok(Table::record(entries))
}

fn schema_enums<const N: usize>(schema: &str, names: &[&str; N]) -> [Vec<String>; N] {
    let schema = Document::parse(schema).unwrap();
    names.map(|name| {
        schema
            .descendants()
            .find(|node| node.attribute("name") == Some(name))
            .unwrap()
            .descendants()
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    })
}

fn effect_table(start: usize) -> Table {
    Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::named(
            "PartitionFilterTestType",
            0x9f0d_a607,
            Target::Data(start + 4),
        ),
        Entry::named("ForbiddenTargetObjectStatus", 0xa7bd_811e, Target::Null),
        Entry::named("ForbiddenTargetModelCondition", 0x67db_bfc9, Target::Null),
        Entry::named("Radius", 0x6458_64f3, Target::Data(start + 0x10)),
        Entry::named("SpecialObjectFilter", 0xe46e_a3b2, Target::Null),
        Entry::named(
            "RequiredUpgrade",
            0x26fa_cfc7,
            Target::Table(Table::array(start + 0x18, [])),
        ),
        Entry::named(
            "ForbiddenUpgrade",
            0x26fa_cfc7,
            Target::Table(Table::array(start + 0x20, [])),
        ),
    ])
}

fn weapon_flags(value: &str, kind: usize, path: &Path) -> io::Result<u32> {
    static FLAGS: OnceLock<[Vec<String>; 3]> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        let schema = Document::parse(include_str!(
            "../../../schemas/ra3/xsd/AssetTypeWeaponTemplate.xsd"
        ))
        .unwrap();
        ["WeaponFlagsType", "WeaponAffectsType", "WpnAntiT"].map(|name| {
            schema
                .descendants()
                .find(|node| node.attribute("name") == Some(name))
                .unwrap()
                .descendants()
                .filter_map(|node| node.attribute("value").map(str::to_owned))
                .collect()
        })
    });
    value.split_ascii_whitespace().try_fold(0, |mask, flag| {
        let bit = flags[kind]
            .iter()
            .position(|name| name == flag)
            .filter(|&bit| bit < 32)
            .ok_or_else(|| invalid(path, format!("invalid weapon flag {flag}")))?;
        Ok(mask | 1 << bit)
    })
}
