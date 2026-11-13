use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, asset_reference, significant_children, validate_attributes};
use super::behaviors_basic::{base, module_base, weapon_slot};
use super::draws::{boolean, fields};
use super::secondary_damage::write_status_flags;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x4317_22a9;
const SLOT_FIELDS: &[(&str, u32, usize)] = &[
    ("ID", 0x7631_77a4, 4),
    ("AllowInterleavedFiring", 0x77e3_f4f7, 0x18),
    ("InterleavedStyle", 0xbe66_9829, 8),
    ("WeaponChoiceCriteria", 0x1d11_0792, 0xc),
    ("AutoChooseWeapon", 0x77e3_f4f7, 0x19),
];
const WEAPON_FIELDS: &[(&str, u32, usize)] = &[
    ("Ordering", 0x1848_c549, 0),
    ("Template", 0xd046_dd22, 4),
    ("Upgrade", 0x4aba_1f29, 8),
    ("ObjectStatus", 0xa7bd_811e, 0xc),
    ("ForbiddenObjectStatus", 0xa7bd_811e, 0x28),
    ("IsPlayerUpgradePermanent", 0x77e3_f4f7, 0x44),
    ("ConquerMetricsAlwaysCount", 0x77e3_f4f7, 0x45),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    chooser_relocations: &mut Vec<(u32, u32)>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id"], path)?;
    let mut groups: [Vec<_>; 4] = std::array::from_fn(|_| Vec::new());
    let slots = [
        ("WeaponSlotHardpoint", 0xce58_53d7),
        ("WeaponSlotTurret", 0x4f14_a301),
        ("WeaponSlotHierarchicalTurret", 0xc488_4051),
        ("WeaponSlotContained", 0xba2d_72e2),
    ];
    for child in significant_children(node) {
        let index = slots
            .iter()
            .position(|&(name, _)| child.has_tag_name((SAGE_NS, name)))
            .ok_or_else(|| {
                invalid(
                    path,
                    format!(
                        "unexpected WeaponSetUpdate child {}",
                        child.tag_name().name()
                    ),
                )
            })?;
        groups[index].push(child);
    }
    let start = data.allocate(pointer, 0x28)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (index, ((name, kind), nodes)) in slots.into_iter().zip(groups).enumerate() {
        let count = start + 8 + index * 8;
        let size = [0x1c, 0x7c, 0x80, 0x1c][index];
        let first = data.list(count, nodes.len(), size)?;
        let mut items = Vec::with_capacity(nodes.len());
        for (index, child) in nodes.into_iter().enumerate() {
            let start = first + index * size;
            let slot = encode_slot(child, start, data, references, reference_types)?;
            if matches!(name, "WeaponSlotTurret" | "WeaponSlotHierarchicalTurret") {
                let chooser = start + 0x74;
                let weapons = start + 0x14;
                if data.payload[chooser..chooser + 4] != [0; 4]
                    && data.payload[weapons..weapons + 4] != [0; 4]
                {
                    chooser_relocations.push((weapons as u32, chooser as u32));
                }
            }
            let slot = if name == "WeaponSlotContained" {
                Table::record(vec![base(0xce58_53d7, slot)])
            } else {
                slot
            };
            items.push((0, Target::Table(slot)));
        }
        entries.push(Entry::named(
            name,
            kind,
            Target::Table(Table::array(count, items)),
        ));
    }
    Ok((KIND, Table::record(entries)))
}

fn encode_slot(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let hierarchy = node.has_tag_name((SAGE_NS, "WeaponSlotHierarchicalTurret"));
    let turret = hierarchy || node.has_tag_name((SAGE_NS, "WeaponSlotTurret"));
    let allowed: Vec<_> = SLOT_FIELDS
        .iter()
        .map(|&(name, _, _)| name)
        .chain(hierarchy.then_some("ParentID"))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let id = node
        .attribute("ID")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid weapon slot ID"))?;
    data.word(start + 4, id);
    data.payload[start + 0x18] = boolean(node, "AllowInterleavedFiring", false, path)?;
    data.payload[start + 0x19] = boolean(node, "AutoChooseWeapon", true, path)?;
    use super::super::global_ui::enum_value;
    data.word(
        start + 8,
        enum_value(
            node.attribute("InterleavedStyle")
                .unwrap_or("INTERLEAVE_RANDOM"),
            &["INTERLEAVE_FIRST_AVAILABLE", "INTERLEAVE_RANDOM"],
            path,
        )?,
    );
    data.word(
        start + 0xc,
        enum_value(
            node.attribute("WeaponChoiceCriteria")
                .unwrap_or("PREFER_MOST_DAMAGE"),
            &[
                "PREFER_MOST_DAMAGE",
                "PREFER_LONGEST_RANGE",
                "PREFER_GRAB_OVER_DAMAGE",
                "PREFER_LEAST_MOVEMENT",
                "SELECT_AT_RANDOM",
                "USE_WEAPONSET_DEFAULT_CRITERIA",
            ],
            path,
        )?,
    );
    let allowed: Vec<_> = WEAPON_FIELDS.iter().map(|&(name, _, _)| name).collect();
    let mut weapons = Vec::new();
    let mut children = significant_children(node).peekable();
    while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Weapon"))) {
        if has_content(child) {
            return Err(invalid(path, "expected a leaf Weapon in weapon slot"));
        }
        validate_attributes(child, &allowed, path)?;
        let mut ordinals = [0; 2];
        for attr in child.attributes() {
            let (index, kind) = match attr.name() {
                "Template" => (0, "WeaponTemplate"),
                "Upgrade" => (1, "UpgradeTemplate"),
                _ => continue,
            };
            let value = attr.value().trim();
            if !value.is_empty() {
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, kind, path)?;
                references.push(id.into());
                reference_types.push(id.type_id);
                ordinals[index] = references.len() as u32;
            }
        }
        let ordering = weapon_slot(
            child.attribute("Ordering").unwrap_or("PRIMARY_WEAPON"),
            path,
        )?;
        weapons.push((ordering, child, ordinals));
    }
    let settings = if turret {
        let child = super::super::required_element(&mut children, "TurretSettings", path)?;
        Some(encode_turret(
            child,
            start + 0x1c,
            data,
            references,
            reference_types,
        )?)
    } else {
        None
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected weapon slot child"));
    }
    weapons.sort_by_key(|&(order, _, _)| order);
    let count = start + 0x10;
    let first = data.list(count, weapons.len(), 0x48)?;
    let mut items = Vec::with_capacity(weapons.len());
    for (index, (order, child, [template, upgrade])) in weapons.into_iter().enumerate() {
        let offset = first + index * 0x48;
        data.word(offset, order);
        data.word(offset + 4, template);
        data.word(offset + 8, upgrade);
        for (name, field) in [("ObjectStatus", 0xc), ("ForbiddenObjectStatus", 0x28)] {
            write_status_flags(child.attribute(name).unwrap_or(""), offset + field, data)?;
        }
        data.payload[offset + 0x44] = boolean(child, "IsPlayerUpgradePermanent", false, path)?;
        data.payload[offset + 0x45] = boolean(child, "ConquerMetricsAlwaysCount", false, path)?;
        items.push((
            0,
            Target::Table(Table::record(fields(offset, WEAPON_FIELDS).collect())),
        ));
    }
    let mut entries = vec![Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start))];
    entries.extend(fields(start, SLOT_FIELDS));
    entries.push(Entry::named(
        "Weapon",
        0x7799_4e4b,
        Target::Table(Table::array(count, items)),
    ));
    if let Some(settings) = settings {
        entries = vec![
            base(0xce58_53d7, Table::record(entries)),
            Entry::named("TurretSettings", 0xf675_3063, Target::Table(settings)),
        ];
        if hierarchy {
            let parent_id = node
                .attribute("ParentID")
                .unwrap_or("0")
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid(path, "invalid weapon turret ParentID"))?;
            data.word(start + 0x7c, parent_id);
            entries = vec![
                base(0x4f14_a301, Table::record(entries)),
                Entry::named("ParentID", 0x7631_77a4, Target::Data(start + 0x7c)),
            ];
        }
    }
    Ok(Table::record(entries))
}

fn encode_turret(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const REAL: u32 = 0x6458_64f3;
    const BOOL: u32 = 0x77e3_f4f7;
    const ANGLE: u32 = 0x4e16_ea4d;
    const TIME: u32 = 0x9986_69e8;
    const SLOTS: u32 = 0x28ae_db15;
    const STATUS: u32 = 0xa7bd_811e;
    const FIELDS: &[(&str, u32, usize, &str)] = &[
        ("TurretTurnRate", REAL, 0, "0"),
        ("TurretPitchRate", REAL, 4, "0"),
        ("AllowsPitch", BOOL, 0x5c, "false"),
        ("MinimumPitch", ANGLE, 8, "0d"),
        ("PitchHeight", REAL, 0xc, "50"),
        ("ControlledWeaponSlots", SLOTS, 0x10, ""),
        ("InvalidIdleScanObjectStatus", STATUS, 0x14, ""),
        ("IdleTurnRateModifier", REAL, 0x30, "50"),
        ("MinIdleScanTime", TIME, 0x34, "0s"),
        ("MaxIdleScanTime", TIME, 0x38, "0s"),
        ("MinIdleScanAngle", REAL, 0x3c, "0"),
        ("MaxIdleScanAngle", REAL, 0x40, "0"),
        ("MaxDeflectionClockwise", ANGLE, 0x44, "180d"),
        ("MaxDeflectionAntiClockwise", ANGLE, 0x48, "180d"),
        ("NaturalTurretAngle", ANGLE, 0x4c, "0d"),
        ("RecenterWhenOutOfTurnRange", BOOL, 0x5d, "true"),
        ("BlindFireDistance", REAL, 0x50, "0"),
        ("ComeToHaltJiggle", ANGLE, 0x54, "0d"),
        ("SetAngleModelStates", BOOL, 0x5e, "false"),
    ];
    let path = data.path;
    let allowed: Vec<_> = FIELDS.iter().map(|&(name, ..)| name).collect();
    validate_attributes(node, &allowed, path)?;
    let mut entries = Vec::with_capacity(FIELDS.len() + 1);
    for &(name, kind, field, default) in FIELDS {
        let offset = start + field;
        let value = node.attribute(name).unwrap_or(default);
        match kind {
            BOOL => data.payload[offset] = boolean(node, name, default == "true", path)?,
            STATUS => write_status_flags(value, offset, data)?,
            _ => {
                let word = match kind {
                    ANGLE => super::slow_death::angle(value, name, data)?.to_bits(),
                    TIME => super::super::sage_time(value, name, path)?.to_bits(),
                    SLOTS => {
                        let mut flags = 0;
                        for slot in value.split_ascii_whitespace() {
                            flags |= 1 << weapon_slot(slot, path)?;
                        }
                        flags
                    }
                    REAL if matches!(name, "PitchHeight" | "IdleTurnRateModifier") => {
                        super::super::percentage(value, name, path)?.to_bits()
                    }
                    REAL => super::super::finite_number(value, name, path)?.to_bits(),
                    _ => unreachable!(),
                };
                data.word(offset, word);
            }
        }
        entries.push(Entry::named(name, kind, Target::Data(offset)));
    }
    let mut children = significant_children(node).peekable();
    let name = "TurretAITargetChooserData";
    let chooser = if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name)))
    {
        Target::Table(super::ai_update::encode_chooser(
            child,
            start + 0x58,
            data,
            references,
            reference_types,
        )?)
    } else {
        Target::Null
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected TurretSettings child"));
    }
    entries.push(Entry::named(name, 0x08cf_34b0, chooser));
    Ok(Table::record(entries))
}
