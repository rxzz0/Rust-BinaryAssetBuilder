use super::binary::BinaryData;
use super::tokenized::{self, Entry, Table, Target};
use super::{
    Marshaled, asset_reference, finite_number, percentage, sage_angle, sage_time,
    significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::{io, path::Path, sync::OnceLock};

mod damage_variants;
mod misc_variants;

// Read the fixed token table and default fields from the empty EA fixture.
const BASE: &[u8] = include_bytes!(
    "../../../tests/fixtures/ea-batches/weapon-template-root-assets/94d4d96e.0ce6168e.a529f9d4.226acfbb.asset"
);

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ROOT: usize = 0x514;
    let mut data = BinaryData {
        payload: BASE[32 + ROOT..].to_vec(),
        relocations: Vec::new(),
        path,
    };
    let mut table = Table::from_layout(&BASE[32..32 + ROOT], &data.payload, path)?;
    let mut out = Marshaled::default();
    encode_root_attributes(node, &mut table, &mut data, &mut out)?;
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
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "WeaponAiHintInfo")))
    {
        let hints = encode_ai_hints(child, &mut data, &mut out)?;
        if !table.replace_target("WeaponAiHintInfo", Target::Table(hints)) {
            return Err(invalid(path, "weapon layout missing WeaponAiHintInfo"));
        }
    }
    let mut derived_relocations = Vec::new();
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Nuggets"))) {
        let nuggets = encode_nuggets(child, &mut data, &mut out, &mut derived_relocations)?;
        if !table.replace_target("Nuggets", Target::Table(nuggets)) {
            return Err(invalid(path, "weapon layout missing Nuggets"));
        }
    }
    let mut scatter_radii = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ScatterRadiusVsType")))
    {
        scatter_radii.push(child);
    }
    if !scatter_radii.is_empty() {
        let radii = encode_scatter_radii(&scatter_radii, &mut data, &mut out)?;
        if !table.replace_target("ScatterRadiusVsType", Target::Table(radii)) {
            return Err(invalid(path, "weapon layout missing ScatterRadiusVsType"));
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
    // These layouts emit derived pointers before their inherited effect pointers.
    for (start, derived_start, end) in derived_relocations {
        let first = data.relocations.partition_point(|&offset| offset < start);
        let derived = data
            .relocations
            .partition_point(|&offset| offset < derived_start);
        let end = data.relocations.partition_point(|&offset| offset < end);
        data.relocations[first..end].rotate_left(derived - first);
    }
    tokenized::finish(table, data, out)
}

fn encode_scatter_radii(
    nodes: &[Node<'_, '_>],
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    use super::game_object::{encode_inline_object_filter, write_model_conditions};

    const COUNT: usize = 0x110;
    const ENTRY_SIZE: usize = 0xb0;
    let path = data.path;
    let start = data.list(COUNT, nodes.len(), ENTRY_SIZE)?;
    let mut entries = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().copied().enumerate() {
        let record = start + index * ENTRY_SIZE;
        validate_attributes(node, &["Radius", "RequiredModelConditions"], path)?;
        let mut children = significant_children(node);
        let filter = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, "Filter")))
            .ok_or_else(|| invalid(path, "ScatterRadiusVsType requires Filter"))?;
        if children.next().is_some() {
            return Err(invalid(path, "unexpected ScatterRadiusVsType child"));
        }
        let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
        data.word(record, radius.to_bits());
        write_model_conditions(
            node.attribute("RequiredModelConditions").unwrap_or(""),
            record + 4,
            data,
        )?;
        let filter =
            encode_inline_object_filter(filter, record + 0x40, data, &mut out.reference_types)?;
        entries.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Radius", 0x6458_64f3, Target::Data(record)),
                Entry::named(
                    "RequiredModelConditions",
                    0x67db_bfc9,
                    Target::Data(record + 4),
                ),
                Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)),
            ])),
        ));
    }
    Ok(Table::array(COUNT, entries))
}

fn encode_ai_hints(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    let path = data.path;
    const WARHEAD: &str = "UseAsWarheadForDamageCalculations";
    validate_attributes(
        node,
        &[
            "IsAntiGarrisonWeapon",
            "MaxSpeedOfTarget",
            "UseLongLockOnTimeCode",
            WARHEAD,
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected WeaponAiHintInfo content"));
    }
    let start = data.allocate(0xf8, 12)?;
    let warhead = nugget_reference(node.attribute(WARHEAD), "WeaponTemplate", path, out)?;
    data.word(start + 4, warhead);
    data.payload[start + 8] = flag(node, "IsAntiGarrisonWeapon", false, path)?;
    data.payload[start + 9] = flag(node, "UseLongLockOnTimeCode", false, path)?;
    let speed = if let Some(value) = node.attribute("MaxSpeedOfTarget") {
        let offset = data.allocate(start, 4)?;
        data.word(
            offset,
            finite_number(value, "MaxSpeedOfTarget", path)?.to_bits(),
        );
        Target::Data(offset)
    } else {
        Target::Null
    };
    Ok(Table::record(vec![
        Entry::named("IsAntiGarrisonWeapon", 0x77e3_f4f7, Target::Data(start + 8)),
        Entry::named("MaxSpeedOfTarget", 0x6458_64f3, speed),
        Entry::named(
            "UseLongLockOnTimeCode",
            0x77e3_f4f7,
            Target::Data(start + 9),
        ),
        Entry::named(WARHEAD, 0xd046_dd22, Target::Data(start + 4)),
    ]))
}

const WEAPON_SCHEMA: &str = include_str!("../../../schemas/ra3/xsd/AssetTypeWeaponTemplate.xsd");

fn encode_root_attributes(
    node: Node<'_, '_>,
    table: &mut Table,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    static ATTRIBUTES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    let attributes = ATTRIBUTES.get_or_init(|| {
        let schema = Document::parse(WEAPON_SCHEMA).expect("bundled weapon schema must be valid");
        schema
            .descendants()
            .find(|field| {
                field.has_tag_name(("http://www.w3.org/2001/XMLSchema", "complexType"))
                    && field.attribute("name") == Some("WeaponTemplate")
            })
            .expect("bundled WeaponTemplate schema must exist")
            .descendants()
            .filter(|field| field.has_tag_name(("http://www.w3.org/2001/XMLSchema", "attribute")))
            .map(|field| {
                (
                    field.attribute("name").unwrap().to_owned(),
                    field.attribute("type").unwrap().to_owned(),
                )
            })
            .collect()
    });
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(attributes.iter().map(|(name, _)| name.as_str()))
        .collect();
    validate_attributes(node, &allowed, path)?;
    for (name, kind) in attributes {
        let Some(raw) = node.attribute(name.as_str()) else {
            continue;
        };
        let optional = match name.as_str() {
            "FireSound" => Some((0x50, 4)),
            "FireSoundPerClip" => Some((0x54, 4)),
            "FiringLoopSound" => Some((0x58, 4)),
            "RequiredFiringObjectStatus" => Some((0x84, 0x1c)),
            "ForbiddenFiringObjectStatus" => Some((0x88, 0x1c)),
            "PreAttackWeapon" => Some((0xdc, 4)),
            _ => None,
        };
        let offset = if let Some((pointer, size)) = optional {
            let target = data.allocate(pointer, size)?;
            if !table.replace_target(name, Target::Data(target)) {
                return Err(invalid(path, format!("weapon layout missing {name}")));
            }
            target
        } else {
            table
                .data_offset(name)
                .ok_or_else(|| invalid(path, format!("weapon layout missing {name}")))?
        };
        let word = match kind.as_str() {
            "SageBool" => {
                data.payload[offset] = match raw.trim() {
                    "true" | "1" => 1,
                    "false" | "0" => 0,
                    _ => return Err(invalid(path, format!("invalid WeaponTemplate {name}"))),
                };
                continue;
            }
            "xs:string" => {
                data.string(offset, raw)?;
                continue;
            }
            "ObjectStatusBitFlags" => {
                super::game_object::write_status_flags(raw, offset, data)?;
                continue;
            }
            "SageReal" => finite_number(raw, name, path)?.to_bits(),
            "Angle" => sage_angle(raw, name, path)?.to_bits(),
            "Time" => sage_time(raw, name, path)?.to_bits(),
            "Percentage" => percentage(raw, name, path)?.to_bits(),
            "SageInt" => {
                raw.trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))? as u32
            }
            "WeaponFlagsBitFlags" => weapon_flags(raw, 0, path)?,
            "WeaponAffectsBitFlags" => weapon_flags(raw, 1, path)?,
            "WeaponAntiBitFlags" => weapon_anti_flags(raw, path)?,
            "WeaponCollideBitFlags" => weapon_flags(raw, 3, path)?,
            "WeaponPrefireType"
            | "WeaponReAcquireDetailType"
            | "WeaponReloadType"
            | "VirtualDamageType"
            | "FXTriggerType" => weapon_enum(raw, kind, path)?,
            "AudioEventInfoRef" => super::game_object::audio_reference(
                raw,
                path,
                &mut out.references,
                &mut out.reference_types,
            )?,
            "FXListRef" | "WeaponRef" | "GameObjectWeakRef" => {
                if raw.trim().is_empty() {
                    continue;
                }
                let ty = match kind.as_str() {
                    "FXListRef" => "FXList",
                    "WeaponRef" => "WeaponTemplate",
                    _ => "GameObject",
                };
                let value = raw.split_once('\\').map_or(raw, |(value, _)| value);
                let id = asset_reference(value, ty, path)?;
                out.reference_types.push(id.type_id);
                if kind == "GameObjectWeakRef" {
                    id.instance_id
                } else {
                    out.references.push(id.into());
                    out.references.len() as u32
                }
            }
            _ => {
                return Err(invalid(
                    path,
                    format!("unsupported WeaponTemplate attribute type {kind}"),
                ));
            }
        };
        data.word(offset, word);
    }
    Ok(())
}

fn weapon_enum(raw: &str, kind: &str, path: &Path) -> io::Result<u32> {
    const NAMES: [&str; 4] = [
        "WeaponPrefireType",
        "WeaponReAcquireDetailType",
        "WeaponReloadType",
        "VirtualDamageType",
    ];
    static ENUMS: OnceLock<[Vec<String>; 4]> = OnceLock::new();
    static FX: OnceLock<[Vec<String>; 1]> = OnceLock::new();
    let values = if kind == "FXTriggerType" {
        &FX.get_or_init(|| {
            schema_enums(
                include_str!("../../../schemas/ra3/xsd/Modules/BaseModules.xsd"),
                &["FXTriggerType"],
            )
        })[0]
    } else {
        let index = NAMES
            .iter()
            .position(|&name| name == kind)
            .expect("known weapon enum type");
        &ENUMS.get_or_init(|| schema_enums(WEAPON_SCHEMA, &NAMES))[index]
    };
    super::ai_strategic::enum_index(raw.trim(), values, kind, path)
}

const LINE_DAMAGE: u32 = 0xefaf_6340;

fn encode_nuggets(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
    derived_relocations: &mut Vec<(u32, u32, u32)>,
) -> io::Result<Table> {
    validate_attributes(node, &[], data.path)?;
    let children: Vec<_> = significant_children(node).collect();
    let first = data.list(0xfc, children.len(), 4)?;
    let mut entries = Vec::with_capacity(children.len());
    for (index, child) in children.into_iter().enumerate() {
        let pointer = first + index * 4;
        if child
            .children()
            .any(|node| node.has_tag_name((SAGE_NS, "DamageScalarDetails")))
        {
            let start = data.payload.len() as u32;
            derived_relocations.push((start, start + 0x88, start + 0x90));
        }
        let variant = damage_variants::Variant::for_node(child);
        let (kind, target) = match (child.tag_name().namespace(), child.tag_name().name()) {
            (Some(SAGE_NS), "FireOnObjectsNugget") => {
                (0x7814_daf0, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "TintObjectsNugget") => {
                (0xda74_d739, tint_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "DOTNugget") => {
                (0xefbb_5226, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "DamageNugget") => {
                (0xc0f5_3e99, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "LineDamageNugget") => {
                (LINE_DAMAGE, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "DamageContainedNugget") => {
                (0x929b_5398, fire_on_objects(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "MetaImpactNugget") => {
                let start = data.payload.len() as u32;
                derived_relocations.push((start, start + 0x28, start + 0x60));
                (META_IMPACT, meta_impact(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "ParalyzeNugget") => {
                (0x0a19_eadb, paralyze_nugget(child, pointer, data, out)?)
            }
            (Some(SAGE_NS), "AttributeModifierNugget") => (
                0xe3c2_d15b,
                attribute_modifier_nugget(child, pointer, data, out)?,
            ),
            (Some(SAGE_NS), "ProjectileNugget") => {
                let start = data.payload.len() as u32;
                derived_relocations.push((start, start + 0x28, start + 0x44));
                (
                    0x87c7_bdcc,
                    simple_reference_nugget(child, pointer, data, out, true)?,
                )
            }
            (Some(SAGE_NS), "WeaponOCLNugget") => (
                0xc606_7c3f,
                simple_reference_nugget(child, pointer, data, out, false)?,
            ),
            (Some(SAGE_NS), "LuaEventNugget") => {
                let start = data.payload.len() as u32;
                derived_relocations.push((start, start + 0x28, start + 0x34));
                misc_variants::encode(child, pointer, data, out)?
            }
            (Some(SAGE_NS), "SeedTiberiumNugget") => {
                misc_variants::encode(child, pointer, data, out)?
            }
            (Some(SAGE_NS), _) if let Some(variant) = variant => (
                variant.type_id(),
                fire_on_objects(child, pointer, data, out)?,
            ),
            (Some(SAGE_NS), name) => {
                let (_, kind, fields) = SIMPLE_EFFECTS
                    .iter()
                    .find(|(module, _, _)| *module == name)
                    .ok_or_else(|| {
                        invalid(data.path, format!("unsupported weapon nugget {name}"))
                    })?;
                (
                    *kind,
                    simple_effect(child, pointer, data, out, *kind, fields)?,
                )
            }
            _ => return Err(invalid(data.path, "unexpected weapon nugget namespace")),
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

enum EffectValue {
    Real,
    Percentage,
    Angle,
    Time,
    Integer,
    Boolean,
    ModuleId,
    DamageEnum(usize, u32),
    InfoWarFlags,
    Reference(&'static str, u32),
}

use EffectValue::{
    Angle, Boolean, DamageEnum, InfoWarFlags, Integer, ModuleId, Percentage, Real, Reference, Time,
};

type EffectField = (&'static str, EffectValue, Option<&'static str>);
const LIFETIME: EffectField = ("Lifetime", Time, Some("1s"));
const SIMPLE_EFFECTS: &[(&str, u32, &[EffectField])] = &[
    (
        "AttachNugget",
        0x2255_aed4,
        &[("AttachModuleId", ModuleId, None)],
    ),
    (
        "StripMaxHealthPercentNugget",
        0x60af_0859,
        &[
            ("AmountToStrip", Percentage, Some("50%")),
            ("DamageType", DamageEnum(0, 0x4648_6ff1), Some("UNDEFINED")),
            ("DeathType", DamageEnum(1, 0xc608_ed36), Some("NORMAL")),
            (
                "DamageFXType",
                DamageEnum(2, 0x2faa_4e62),
                Some("UNDEFINED"),
            ),
        ],
    ),
    (
        "InformationWarfareNugget",
        0x3a4d_3175,
        &[
            ("InfoWarType", InfoWarFlags, Some("NONE")),
            ("RadarJamRadius", Real, Some("0")),
            ("RadarJamDuration", Time, Some("0s")),
        ],
    ),
    (
        "ActivateLaserNugget",
        0xe960_34a2,
        &[
            LIFETIME,
            ("LaserId", Integer, Some("0")),
            ("HitGroundFX", Reference("FXList", 0x0d1c_8e83), Some("")),
            ("OverShootDistance", Real, Some("0")),
        ],
    ),
    (
        "ActivateStreamNugget",
        0x80ba_8a54,
        &[LIFETIME, ("StreamId", Integer, Some("0"))],
    ),
    ("ActivateLiftObjectNugget", 0xa514_e7a2, &[LIFETIME]),
    ("ActivateLinearDamageNugget", 0xdc3b_ac7f, &[LIFETIME]),
    ("ActivateCircularDamageNugget", 0x8cab_3f5b, &[LIFETIME]),
    (
        "SpecialPowerNugget",
        0x5be9_57a9,
        &[(
            "SpecialPowerTemplate",
            Reference("SpecialPowerTemplate", 0xaee8_3888),
            Some(""),
        )],
    ),
    ("SlavesAttackNugget", 0xb976_dc1f, &[]),
    ("ReportWeaponFiredStatNugget", 0x63d9_0f36, &[]),
    ("ContainedObjectAttackNugget", 0x2457_5e0b, &[]),
    (
        "TerrainCraterNugget",
        0xd1d0_57d3,
        &[("Depth", Real, Some("0")), ("Lift", Percentage, Some("0%"))],
    ),
    (
        "ScatterProjectileNugget",
        0xf642_22be,
        &[
            ("ScatterMin", Real, Some("0")),
            ("ScatterMax", Real, Some("0")),
        ],
    ),
    (
        "InformTargetNugget",
        0x4280_e5f8,
        &[
            ("MinTimeToImpactFudgeFactor", Real, Some("0.85")),
            ("MaxTimeToImpactFudgeFactor", Real, Some("1.15")),
        ],
    ),
    (
        "SuppressionNugget",
        0xf4a6_ada7,
        &[
            ("Suppression", Real, Some("0")),
            ("DurationSeconds", Time, Some("0s")),
            ("SuppressionTaperOff", Real, Some("-1")),
            ("SuppressionArc", Angle, Some("360d")),
            ("SuppressionArcInverted", Boolean, Some("false")),
        ],
    ),
    (
        "SecondaryDamageNugget",
        0x9387_27a3,
        &[
            ("Amount", Integer, None),
            ("DamageTaperOff", Real, Some("-1")),
            ("PercentDamageToContained", Percentage, Some("0%")),
        ],
    ),
    (
        "SpendStolenTiberiumNugget",
        0xff7b_0869,
        &[("AmountToSpend", Integer, Some("1"))],
    ),
];

fn simple_effect(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
    kind: u32,
    fields: &[EffectField],
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = fields.iter().map(|(name, ..)| *name).collect();
    validate_effect_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x28 + fields.len() * 4)?;
    data.word(start, kind);
    let mut entries = Vec::with_capacity(fields.len() + 1);
    for (index, &(name, ref value_type, default)) in fields.iter().enumerate() {
        let raw = node
            .attribute(name)
            .or(default)
            .ok_or_else(|| invalid(path, format!("missing {name}")))?;
        let (kind, value) = match *value_type {
            Real => (0x6458_64f3, finite_number(raw, name, path)?.to_bits()),
            Percentage => (0x6458_64f3, percentage(raw, name, path)?.to_bits()),
            Angle => (0x4e16_ea4d, sage_angle(raw, name, path)?.to_bits()),
            Time => (0x9986_69e8, sage_time(raw, name, path)?.to_bits()),
            Integer => (
                0x23e6_50b8,
                raw.trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))? as u32,
            ),
            Boolean => (
                0x77e3_f4f7,
                u32::from(flag(node, name, raw == "true", path)?),
            ),
            ModuleId => (
                0xbdf0_7965,
                super::hash_name(&raw.trim().to_ascii_lowercase()),
            ),
            DamageEnum(index, token) => (token, damage_enum(index, raw, path)?),
            InfoWarFlags => {
                static FLAGS: OnceLock<[Vec<String>; 1]> = OnceLock::new();
                let flags = FLAGS.get_or_init(|| schema_enums(WEAPON_SCHEMA, &["InfoWarEffect"]));
                let mask = raw.split_ascii_whitespace().try_fold(0, |mask, flag| {
                    super::ai_strategic::enum_index(flag, &flags[0], name, path)
                        .map(|bit| mask | 1 << bit)
                })?;
                (0x0fd4_89e2, mask)
            }
            Reference(kind, token) => (token, nugget_reference(Some(raw), kind, path, out)?),
        };
        let offset = start + 0x28 + index * 4;
        data.word(offset, value);
        entries.push(Entry::named(name, kind, Target::Data(offset)));
    }
    let mut children = significant_children(node).peekable();
    let effect = encode_effect(node, start, data, out, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(
            path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    entries.insert(0, effect);
    Ok(Table::record(entries))
}

fn tint_objects(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    const FLOAT: u32 = 0x6458_64f3;
    const TIME: u32 = 0x9986_69e8;
    let path = data.path;
    validate_effect_attributes(
        node,
        &[
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
    let mut children = significant_children(node).peekable();
    let mut entries = vec![encode_effect(node, start, data, out, &mut children)?];
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
    let contained = node.has_tag_name((SAGE_NS, "DamageContainedNugget"));
    let line = node.has_tag_name((SAGE_NS, "LineDamageNugget"));
    let variant = damage_variants::Variant::for_node(node);
    let is_damage = dot || direct || contained || line || variant.is_some();
    const LINE_FIELDS: &[(&str, u32, usize)] = &[
        ("OffsetAngle", 0x4e16_ea4d, 0x98),
        ("LineWidth", 0x6458_64f3, 0x9c),
        ("LineLengthLeadIn", 0x6458_64f3, 0xa0),
        ("LineLengthLeadOut", 0x6458_64f3, 0xa4),
        ("UseDynamicLineLength", 0x77e3_f4f7, 0xac),
        ("OverShootDistance", 0x6458_64f3, 0xa8),
    ];
    const DAMAGE_REALS: [(&str, usize); 8] = [
        ("DamageTaperOff", 0x2c),
        ("MinRadius", 0x30),
        ("DamageMaxHeight", 0x38),
        ("DamageMaxHeightAboveTerrain", 0x3c),
        ("FlankingBonus", 0x40),
        ("FlankedScalar", 0x44),
        ("DrainLifeMultiplier", 0x5c),
        ("DamageSpeed", 0x60),
    ];
    const DAMAGE_BOOLS: [(&str, usize); 8] = [
        ("DamageArcInverted", 0x90),
        ("AcceptDamageAdd", 0x91),
        ("OnlyKillOwnerWhenTriggered", 0x92),
        ("DrainLife", 0x93),
        ("NotifyOwnerOnVictimDeath", 0x94),
        ("NotifyObserversOnPreDamageEffectPosition", 0x95),
        ("ForceFXPositionToVictim", 0x96),
        ("RadiusAffectsBridges", 0x97),
    ];
    let extended: Vec<_> = [
        "Damage",
        "DamageType",
        "DamageFXType",
        "DeathType",
        "DelayTimeSeconds",
    ]
    .into_iter()
    .chain(DAMAGE_REALS.map(|(name, _)| name))
    .chain(DAMAGE_BOOLS.map(|(name, _)| name))
    .chain(
        dot.then_some(["DamageInterval", "DamageDuration", "RemoveIfHealed"])
            .into_iter()
            .flatten(),
    )
    .chain(
        contained
            .then_some(["MaxUnitsToDamage", "WindowBlastFX"])
            .into_iter()
            .flatten(),
    )
    .chain(
        line.then_some(LINE_FIELDS)
            .into_iter()
            .flatten()
            .map(|&(name, _, _)| name),
    )
    .chain(
        variant
            .into_iter()
            .flat_map(|variant| variant.attributes().iter().copied()),
    )
    .collect();
    validate_effect_attributes(
        node,
        if is_damage {
            &extended
        } else {
            &["Weapon", "FireOnVictimObject", "VictimMustBeAlive"]
        },
        path,
    )?;
    let start = data.allocate(
        pointer,
        if let Some(variant) = variant {
            variant.size()
        } else if contained {
            0x110
        } else if line {
            0xb0
        } else if direct {
            0x98
        } else {
            0xa4
        },
    )?;
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
    if is_damage {
        for (name, offset) in DAMAGE_REALS {
            if let Some(value) = node.attribute(name) {
                data.word(start + offset, finite_number(value, name, path)?.to_bits());
            }
        }
        for (name, offset) in DAMAGE_BOOLS {
            data.payload[start + offset] = flag(node, name, name == "AcceptDamageAdd", path)?;
        }
        if let Some(value) = node.attribute("DelayTimeSeconds") {
            data.word(
                start + 0x48,
                sage_time(value, "DelayTimeSeconds", path)?.to_bits(),
            );
        }
        data.word(
            start,
            if let Some(variant) = variant {
                variant.type_id()
            } else if dot {
                0xefbb_5226
            } else if line {
                LINE_DAMAGE
            } else if contained {
                0x929b_5398
            } else {
                0xc0f5_3e99
            },
        );
        data.word(
            start + 0x28,
            finite_number(node.attribute("Damage").unwrap_or("0"), "Damage", path)?.to_bits(),
        );
        for (index, (name, default)) in [
            ("DamageType", "UNDEFINED"),
            ("DeathType", "NORMAL"),
            ("DamageFXType", "UNDEFINED"),
        ]
        .into_iter()
        .enumerate()
        {
            data.word(
                start + 0x4c + index * 4,
                damage_enum(index, node.attribute(name).unwrap_or(default), path)?,
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
    if contained
        && let Some(value) = node
            .attribute("WindowBlastFX")
            .filter(|v| !v.trim().is_empty())
    {
        let id = asset_reference(
            value.split_once('\\').map_or(value, |(name, _)| name),
            "FXList",
            path,
        )?;
        out.reference_types.push(id.type_id);
        out.references.push(id.into());
        data.word(start + 0x9c, out.references.len() as u32);
    }
    let mut scalar_reference_types = Vec::new();
    let scalars = if is_damage {
        encode_damage_scalars(node, start + 0x88, data, &mut scalar_reference_types)?
    } else {
        Table::array(start + 0x88, [])
    };
    let mut children = significant_children(node).peekable();
    let effect = encode_effect(node, start, data, out, &mut children)?;
    let filter =
        match children.next_if(|child| !is_damage && child.has_tag_name((SAGE_NS, "Filter"))) {
            Some(filter) => Target::Table(super::game_object::encode_object_filter(
                filter,
                start + 0x9c,
                data,
                &mut out.reference_types,
            )?),
            None => Target::Null,
        };
    if is_damage {
        while children
            .next_if(|child| child.has_tag_name((SAGE_NS, "DamageScalarDetails")))
            .is_some()
        {}
    }
    out.reference_types.extend(scalar_reference_types);
    let contained_filter = if contained {
        let child = children
            .next_if(|child| child.has_tag_name((SAGE_NS, "DamageObjectFilter")))
            .ok_or_else(|| invalid(path, "DamageContainedNugget requires DamageObjectFilter"))?;
        Some(super::game_object::encode_inline_object_filter(
            child,
            start + 0xa0,
            data,
            &mut out.reference_types,
        )?)
    } else {
        None
    };
    let derived = variant
        .map(|variant| variant.encode_fields(node, start, data, out, &mut children))
        .transpose()?;
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!(
                "unsupported damage nugget child {}",
                child.tag_name().name()
            ),
        ));
    }
    let mut damage = vec![effect];
    damage.extend(
        DAMAGE_FIELDS
            .iter()
            .map(|&(name, kind, offset)| Entry::new(name, kind, Target::Data(start + offset))),
    );
    damage.push(Entry::new(0x9197_20cd, 0xe4a6_e6e4, Target::Table(scalars)));
    if let Some(derived) = derived {
        let mut fields = vec![Entry::new(
            1,
            0xc0f5_3e99,
            Target::Table(Table::record(damage)),
        )];
        fields.extend(derived);
        return Ok(Table::record(fields));
    }
    if direct {
        return Ok(Table::record(damage));
    }
    if line {
        let mut fields = vec![Entry::new(
            1,
            0xc0f5_3e99,
            Target::Table(Table::record(damage)),
        )];
        for &(name, kind, offset) in LINE_FIELDS {
            let offset = start + offset;
            match name {
                "UseDynamicLineLength" => data.payload[offset] = flag(node, name, false, path)?,
                "OffsetAngle" => data.word(
                    offset,
                    sage_angle(node.attribute(name).unwrap_or("0d"), name, path)?.to_bits(),
                ),
                _ => data.word(
                    offset,
                    finite_number(node.attribute(name).unwrap_or("0"), name, path)?.to_bits(),
                ),
            }
            fields.push(Entry::named(name, kind, Target::Data(offset)));
        }
        return Ok(Table::record(fields));
    }
    if let Some(filter) = contained_filter {
        let max = node
            .attribute("MaxUnitsToDamage")
            .unwrap_or("100")
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, "invalid MaxUnitsToDamage"))?;
        data.word(start + 0x98, max as u32);
        return Ok(Table::record(vec![
            Entry::new(1, 0xc0f5_3e99, Target::Table(Table::record(damage))),
            Entry::named("MaxUnitsToDamage", 0x23e6_50b8, Target::Data(start + 0x98)),
            Entry::named("WindowBlastFX", 0x0d1c_8e83, Target::Data(start + 0x9c)),
            Entry::named("DamageObjectFilter", 0xe46e_a3b2, Target::Table(filter)),
        ]));
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

fn encode_damage_scalars(
    node: Node<'_, '_>,
    count: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    use super::game_object::{encode_inline_object_filter, weak_id_list, weak_xml_reference};

    const ENTRY_SIZE: usize = 0x7c;
    let path = data.path;
    let scalars: Vec<_> = node
        .children()
        .filter(|child| child.has_tag_name((SAGE_NS, "DamageScalarDetails")))
        .collect();
    let start = data.list(count, scalars.len(), ENTRY_SIZE)?;
    let mut entries = Vec::with_capacity(scalars.len());
    for (index, node) in scalars.into_iter().enumerate() {
        let record = start + index * ENTRY_SIZE;
        validate_attributes(node, &["Scalar"], path)?;
        let scalar = percentage(node.attribute("Scalar").unwrap_or("100%"), "Scalar", path)?;
        data.word(record, scalar.to_bits());
        let mut children = significant_children(node);
        let filter = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, "Filter")))
            .ok_or_else(|| invalid(path, "DamageScalarDetails requires Filter"))?;
        let filter = encode_inline_object_filter(filter, record + 4, data, reference_types)?;
        let mut upgrades = Vec::new();
        for child in children {
            if !child.has_tag_name((SAGE_NS, "RequiredUpgrade")) {
                return Err(invalid(path, "unexpected DamageScalarDetails child"));
            }
            upgrades.push(weak_xml_reference(
                child,
                "UpgradeTemplate",
                data,
                reference_types,
            )?);
        }
        let upgrades = weak_id_list(data, record + 0x74, upgrades)?;
        entries.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Scalar", 0x6458_64f3, Target::Data(record)),
                Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)),
                Entry::named("RequiredUpgrade", 0x26fa_cfc7, Target::Table(upgrades)),
            ])),
        ));
    }
    Ok(Table::array(count, entries))
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
        .chain([if projectile {
            "WeaponLaunchBoneSlotOverride"
        } else {
            "TargetAsPrimaryObject"
        }])
        .collect();
    validate_effect_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, if projectile { 0x44 } else { 0x30 })?;
    data.word(start, if projectile { 0x87c7_bdcc } else { 0xc606_7c3f });
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
    let mut children: Vec<_> = significant_children(node).collect();
    let projectile_entries = if projectile {
        let slot = super::game_object::weapon_slot(
            node.attribute("WeaponLaunchBoneSlotOverride")
                .unwrap_or("NO_WEAPON"),
            path,
        )?;
        data.word(start + 0x30, slot);
        let first = children
            .iter()
            .position(|child| {
                child.tag_name().namespace() == Some(SAGE_NS)
                    && matches!(
                        child.tag_name().name(),
                        "AttackOffset" | "VeterancyProjectiles" | "SpecificBarrelOverride"
                    )
            })
            .unwrap_or(children.len());
        let mut derived = children.drain(first..).peekable();
        let type_start = out.reference_types.len();
        let entries = projectile_children(&mut derived, start, data, out)?;
        if let Some(child) = derived.next() {
            return Err(invalid(
                path,
                format!(
                    "unexpected ProjectileNugget child {}",
                    child.tag_name().name()
                ),
            ));
        }
        Some((entries, out.reference_types.split_off(type_start)))
    } else {
        None
    };
    let mut children = children.into_iter().peekable();
    let mut entries = vec![encode_effect(node, start, data, out, &mut children)?];
    entries
        .extend(refs.iter().map(|&(name, kind, offset, _)| {
            Entry::named(name, kind, Target::Data(start + offset))
        }));
    if let Some((fields, reference_types)) = projectile_entries {
        entries.extend(fields);
        out.reference_types.extend(reference_types);
    } else {
        data.payload[start + 0x2c] = flag(node, "TargetAsPrimaryObject", false, path)?;
        entries.push(Entry::named(
            "TargetAsPrimaryObject",
            0x77e3_f4f7,
            Target::Data(start + 0x2c),
        ));
    }
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!(
                "unexpected {} child {}",
                node.tag_name().name(),
                child.tag_name().name()
            ),
        ));
    }
    Ok(Table::record(entries))
}

fn projectile_children<'a, 'input: 'a>(
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<[Entry; 4]> {
    let path = data.path;
    let offset = children.next_if(|child| child.has_tag_name((SAGE_NS, "AttackOffset")));
    let offset = super::game_object::optional_vector(offset, start + 0x34, data)?;
    let mut projectiles = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "VeterancyProjectiles")))
    {
        validate_attributes(child, &["VeterancyLevel", "ProjectileTemplate"], path)?;
        if has_content(child) {
            return Err(invalid(path, "unexpected VeterancyProjectiles content"));
        }
        let level = match child.attribute("VeterancyLevel").unwrap_or("REGULAR") {
            "INVALID" => u32::MAX,
            value => super::global_ui::enum_value(
                value,
                &["REGULAR", "VETERAN", "ELITE", "HEROIC"],
                path,
            )?,
        };
        let ordinal = nugget_reference(
            child.attribute("ProjectileTemplate"),
            "GameObject",
            path,
            out,
        )?;
        projectiles.push((level, ordinal));
    }
    let count = start + 0x38;
    let first = data.list(count, projectiles.len(), 8)?;
    let items = projectiles
        .into_iter()
        .enumerate()
        .map(|(index, (level, ordinal))| {
            let index = first + index * 8;
            data.word(index, level);
            data.word(index + 4, ordinal);
            (
                0,
                Target::Table(Table::record(vec![
                    Entry::named("VeterancyLevel", 0xfb9f_bd6f, Target::Data(index)),
                    Entry::named("ProjectileTemplate", 0xbfab_9075, Target::Data(index + 4)),
                ])),
            )
        })
        .collect::<Vec<_>>();
    let barrel = if let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "SpecificBarrelOverride")))
    {
        let value = super::text_value(child, path)?
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, "invalid SpecificBarrelOverride"))?;
        let offset = data.allocate(start + 0x40, 4)?;
        data.word(offset, value as u32);
        Target::Data(offset)
    } else {
        Target::Null
    };
    Ok([
        Entry::named(
            "WeaponLaunchBoneSlotOverride",
            0x1848_c549,
            Target::Data(start + 0x30),
        ),
        Entry::named("AttackOffset", 0x2936_8077, offset),
        Entry::named(
            "VeterancyProjectiles",
            0x4185_7548,
            Target::Table(Table::array(count, items)),
        ),
        Entry::named("SpecificBarrelOverride", 0x23e6_50b8, barrel),
    ])
}

fn damage_enum(index: usize, raw: &str, path: &Path) -> io::Result<u32> {
    const SCHEMAS: &[(&str, &str)] = &[
        (
            "DamageType",
            include_str!("../../../schemas/ra3/xsd/Includes/DamageDef.xsd"),
        ),
        (
            "DeathType",
            include_str!("../../../schemas/ra3/xsd/Includes/Death.xsd"),
        ),
        (
            "DamageFXType",
            include_str!("../../../schemas/ra3/xsd/Includes/DamageFX.xsd"),
        ),
    ];
    static VALUES: [OnceLock<[Vec<String>; 1]>; 3] = [const { OnceLock::new() }; 3];
    let (name, schema) = SCHEMAS[index];
    let values = VALUES[index].get_or_init(|| schema_enums(schema, &[name]));
    let ordinal = super::ai_strategic::enum_index(raw, &values[0], name, path)?;
    Ok(ordinal.wrapping_sub(u32::from(index == 2)))
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

const EFFECT_ATTRIBUTES: &[&str] = &[
    "id",
    "PartitionFilterTestType",
    "ForbiddenTargetObjectStatus",
    "ForbiddenTargetModelCondition",
    "Radius",
];

fn validate_effect_attributes(node: Node<'_, '_>, derived: &[&str], path: &Path) -> io::Result<()> {
    let allowed: Vec<_> = EFFECT_ATTRIBUTES.iter().chain(derived).copied().collect();
    validate_attributes(node, &allowed, path)
}

fn encode_effect<'a, 'input: 'a>(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
) -> io::Result<Entry> {
    use super::game_object::{
        encode_object_filter, weak_id_list, weak_xml_reference, write_model_conditions,
        write_status_flags,
    };

    let path = data.path;
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
    let statuses = match node.attribute("ForbiddenTargetObjectStatus") {
        Some(raw) => {
            let offset = data.allocate(start + 8, 0x1c)?;
            write_status_flags(raw, offset, data)?;
            Target::Data(offset)
        }
        None => Target::Null,
    };
    let conditions = match node.attribute("ForbiddenTargetModelCondition") {
        Some(raw) => {
            let offset = data.allocate(start + 0xc, 0x3c)?;
            write_model_conditions(raw, offset, data)?;
            Target::Data(offset)
        }
        None => Target::Null,
    };
    let filter =
        match children.next_if(|child| child.has_tag_name((SAGE_NS, "SpecialObjectFilter"))) {
            Some(filter) => Target::Table(encode_object_filter(
                filter,
                start + 0x14,
                data,
                &mut out.reference_types,
            )?),
            None => Target::Null,
        };
    let mut entries = vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::named(
            "PartitionFilterTestType",
            0x9f0d_a607,
            Target::Data(start + 4),
        ),
        Entry::named("ForbiddenTargetObjectStatus", 0xa7bd_811e, statuses),
        Entry::named("ForbiddenTargetModelCondition", 0x67db_bfc9, conditions),
        Entry::named("Radius", 0x6458_64f3, Target::Data(start + 0x10)),
        Entry::named("SpecialObjectFilter", 0xe46e_a3b2, filter),
    ];
    for (name, offset) in [("RequiredUpgrade", 0x18), ("ForbiddenUpgrade", 0x20)] {
        let mut upgrades = Vec::new();
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            upgrades.push(weak_xml_reference(
                child,
                "UpgradeTemplate",
                data,
                &mut out.reference_types,
            )?);
        }
        let table = weak_id_list(data, start + offset, upgrades)?;
        entries.push(Entry::named(name, 0x26fa_cfc7, Target::Table(table)));
    }
    Ok(Entry::new(
        1,
        0xc7c2_cc04,
        Target::Table(Table::record(entries)),
    ))
}

const META_IMPACT: u32 = 0xaf42_89fb;

fn meta_impact(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    const FIELDS: &[(&str, EffectValue, &str, usize)] = &[
        ("ShockWaveAmount", Real, "0", 0x28),
        ("ShockWaveRadius", Real, "0", 0x2c),
        ("ShockWaveArc", Angle, "360d", 0x30),
        ("ShockWaveArcInverted", Boolean, "false", 0x58),
        ("ShockWaveTaperOff", Real, "0", 0x34),
        ("ShockWaveSpeed", Real, "0", 0x38),
        ("ShockWaveZMult", Real, "1", 0x3c),
        ("CyclonicFactor", Real, "0", 0x40),
        ("ShockwaveDelaySeconds", Time, "0s", 0x44),
        ("Suppression", Real, "0", 0x48),
        ("InvertShockWave", Boolean, "false", 0x59),
        ("FlipDirection", Boolean, "false", 0x5a),
        ("OnlyWhenJustDied", Boolean, "false", 0x5b),
        ("ShockWaveClearRadius", Boolean, "false", 0x5c),
        ("ShockWaveClearWaveMult", Real, "2", 0x4c),
        ("ShockWaveClearFlingHeight", Real, "100", 0x50),
    ];
    let path = data.path;
    let attributes: Vec<_> = FIELDS.iter().map(|&(name, ..)| name).collect();
    validate_effect_attributes(node, &attributes, path)?;
    let start = data.allocate(pointer, 0x60)?;
    data.word(start, META_IMPACT);
    let kill_filter = node
        .children()
        .find(|child| child.has_tag_name((SAGE_NS, "KillObjectFilter")))
        .map(|child| {
            super::game_object::encode_object_filter(
                child,
                start + 0x54,
                data,
                &mut out.reference_types,
            )
        })
        .transpose()?;
    let mut children = significant_children(node).peekable();
    let mut entries = vec![encode_effect(node, start, data, out, &mut children)?];
    children.next_if(|child| child.has_tag_name((SAGE_NS, "KillObjectFilter")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected MetaImpactNugget child"));
    }
    for &(name, ref kind, default, offset) in FIELDS {
        let raw = node.attribute(name).unwrap_or(default);
        let field = start + offset;
        let token = match kind {
            Boolean => {
                data.payload[field] = flag(node, name, default == "true", path)?;
                0x77e3_f4f7
            }
            Angle => {
                data.word(field, sage_angle(raw, name, path)?.to_bits());
                0x4e16_ea4d
            }
            Time => {
                data.word(field, sage_time(raw, name, path)?.to_bits());
                0x9986_69e8
            }
            _ => {
                data.word(field, finite_number(raw, name, path)?.to_bits());
                0x6458_64f3
            }
        };
        entries.push(Entry::named(name, token, Target::Data(field)));
    }
    entries.push(Entry::named(
        "KillObjectFilter",
        0xe46e_a3b2,
        kill_filter.map_or(Target::Null, Target::Table),
    ));
    Ok(Table::record(entries))
}

fn paralyze_nugget(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    const FIELDS: &[(&str, u32, usize)] = &[
        ("EffectArc", 0x4e16_ea4d, 0x28),
        ("DurationSeconds", 0x9986_69e8, 0x2c),
        ("ParalyzeType", 0x0744_db75, 0x30),
        ("RemoveParalyzeType", 0x0744_db75, 0x34),
        ("ParalyzeFX", 0x0d1c_8e83, 0x38),
    ];
    let path = data.path;
    validate_effect_attributes(
        node,
        &FIELDS.iter().map(|&(name, _, _)| name).collect::<Vec<_>>(),
        path,
    )?;
    let start = data.allocate(pointer, 0x3c)?;
    data.word(start, 0x0a19_eadb);
    let mut entries = Vec::with_capacity(FIELDS.len() + 1);
    for &(name, _, offset) in FIELDS {
        let value = match name {
            "EffectArc" => {
                sage_angle(node.attribute(name).unwrap_or("360d"), name, path)?.to_bits()
            }
            "DurationSeconds" => {
                sage_time(node.attribute(name).unwrap_or("0s"), name, path)?.to_bits()
            }
            "ParalyzeFX" => nugget_reference(node.attribute(name), "FXList", path, out)?,
            _ => {
                static EFFECTS: OnceLock<[Vec<String>; 1]> = OnceLock::new();
                let effects =
                    EFFECTS.get_or_init(|| schema_enums(WEAPON_SCHEMA, &["ParalyzeEffectType"]));
                super::ai_strategic::enum_index(
                    node.attribute(name).unwrap_or("NONE"),
                    &effects[0],
                    name,
                    path,
                )?
            }
        };
        data.word(start + offset, value);
    }
    let mut children = significant_children(node).peekable();
    entries.push(encode_effect(node, start, data, out, &mut children)?);
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ParalyzeNugget child"));
    }
    entries.extend(
        FIELDS
            .iter()
            .map(|&(name, kind, offset)| Entry::named(name, kind, Target::Data(start + offset))),
    );
    Ok(Table::record(entries))
}

fn attribute_modifier_nugget(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<Table> {
    const FIELDS: &[(&str, u32, usize)] = &[
        ("AttributeModifierName", 0x09dd_cff3, 0x28),
        ("AttributeModifierOwnerName", 0x09dd_cff3, 0x2c),
        ("DamageFXType", 0x2faa_4e62, 0x30),
        ("DamageArc", 0x4e16_ea4d, 0x34),
        ("AntiCategories", 0x45f1_59e8, 0x38),
        ("AntiFX", 0x0d1c_8e83, 0x3c),
    ];
    let path = data.path;
    validate_effect_attributes(
        node,
        &FIELDS.iter().map(|&(name, _, _)| name).collect::<Vec<_>>(),
        path,
    )?;
    let start = data.allocate(pointer, 0x40)?;
    data.word(start, 0xe3c2_d15b);
    for &(name, _, offset) in FIELDS {
        let raw = node.attribute(name);
        let value = match name {
            "AttributeModifierName" | "AttributeModifierOwnerName" => {
                nugget_reference(raw, "AttributeModifier", path, out)?
            }
            "DamageFXType" => {
                static VALUES: OnceLock<[Vec<String>; 1]> = OnceLock::new();
                let values = VALUES.get_or_init(|| {
                    schema_enums(
                        include_str!("../../../schemas/ra3/xsd/Includes/DamageFX.xsd"),
                        &["DamageFXType"],
                    )
                });
                raw.map_or(Ok(0), |raw| {
                    super::ai_strategic::enum_index(raw, &values[0], name, path)
                        .map(|index| index.wrapping_sub(1))
                })?
            }
            "DamageArc" => sage_angle(raw.unwrap_or("360d"), name, path)?.to_bits(),
            "AntiCategories" => super::game_object::category_flags(raw.unwrap_or(""), name, path)?,
            "AntiFX" => nugget_reference(raw, "FXList", path, out)?,
            _ => unreachable!(),
        };
        data.word(start + offset, value);
    }
    let mut children = significant_children(node).peekable();
    let mut entries = vec![encode_effect(node, start, data, out, &mut children)?];
    if children.next().is_some() {
        return Err(invalid(path, "unexpected AttributeModifierNugget child"));
    }
    entries.extend(
        FIELDS
            .iter()
            .map(|&(name, kind, offset)| Entry::named(name, kind, Target::Data(start + offset))),
    );
    Ok(Table::record(entries))
}

fn nugget_reference(
    raw: Option<&str>,
    kind: &str,
    path: &Path,
    out: &mut Marshaled,
) -> io::Result<u32> {
    let Some(raw) = raw.filter(|value| !value.trim().is_empty()) else {
        return Ok(0);
    };
    let value = raw.split_once('\\').map_or(raw, |(name, _)| name);
    let id = asset_reference(value, kind, path)?;
    out.references.push(id.into());
    out.reference_types.push(id.type_id);
    Ok(out.references.len() as u32)
}

pub(super) fn weapon_anti_flags(value: &str, path: &Path) -> io::Result<u32> {
    weapon_flags(value, 2, path)
}

fn weapon_flags(value: &str, kind: usize, path: &Path) -> io::Result<u32> {
    static FLAGS: OnceLock<[Vec<String>; 4]> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        schema_enums(
            WEAPON_SCHEMA,
            &[
                "WeaponFlagsType",
                "WeaponAffectsType",
                "WpnAntiT",
                "WeaponCollideType",
            ],
        )
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
