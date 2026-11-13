use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, asset_reference, significant_children, validate_attributes};
use super::behaviors_basic::{module_base, weapon_slot};
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
        if matches!(index, 1 | 2) {
            return Err(invalid(
                path,
                format!(
                    "unsupported WeaponSetUpdate slot {}",
                    child.tag_name().name()
                ),
            ));
        }
        groups[index].push(child);
    }
    let start = data.allocate(pointer, 0x28)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (index, ((name, kind), nodes)) in slots.into_iter().zip(groups).enumerate() {
        let count = start + 8 + index * 8;
        let first = data.list(count, nodes.len(), 0x1c)?;
        let mut items = Vec::with_capacity(nodes.len());
        for (index, child) in nodes.into_iter().enumerate() {
            let slot = encode_slot(
                child,
                first + index * 0x1c,
                data,
                references,
                reference_types,
            )?;
            let slot = if name == "WeaponSlotContained" {
                Table::record(vec![super::behaviors_basic::base(0xce58_53d7, slot)])
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
    let allowed: Vec<_> = SLOT_FIELDS.iter().map(|&(name, _, _)| name).collect();
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
    for child in significant_children(node) {
        if !child.has_tag_name((SAGE_NS, "Weapon")) || has_content(child) {
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
    Ok(Table::record(entries))
}
