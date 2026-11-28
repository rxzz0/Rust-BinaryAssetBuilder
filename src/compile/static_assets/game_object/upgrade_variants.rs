use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, sage_uint, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, upgrade_fields};
use super::draws::{boolean, fields};
use super::passive_area::reference;
use super::spawn::{UPGRADE, id_list, weak_reference};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn encode_fire_when_damaged(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x0897_b555;
    const WEAPON: u32 = 0xd046_dd22;
    const FIELDS: &[(&str, u32, usize)] = &[
        ("InitiallyActive", 0x77e3_f4f7, 0x4c),
        ("DamageTypes", 0x7631_77a4, 0x24),
        ("DamageAmount", 0x6458_64f3, 0x28),
        ("ReactionWeaponPristine", WEAPON, 0x2c),
        ("ReactionWeaponDamaged", WEAPON, 0x30),
        ("ReactionWeaponReallyDamaged", WEAPON, 0x34),
        ("ReactionWeaponRubble", WEAPON, 0x38),
        ("ContinuousWeaponPristine", WEAPON, 0x3c),
        ("ContinuousWeaponDamaged", WEAPON, 0x40),
        ("ContinuousWeaponReallyDamaged", WEAPON, 0x44),
        ("ContinuousWeaponRubble", WEAPON, 0x48),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().chain(FIELDS).map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x50)?;
    for attr in node.attributes() {
        if let Some(&(_, _, offset)) = FIELDS
            .iter()
            .find(|&&(name, token, _)| name == attr.name() && token == WEAPON)
        {
            let ordinal = reference(
                attr.value(),
                "WeaponTemplate",
                path,
                references,
                reference_types,
            )?;
            data.word(start + offset, ordinal);
        }
    }
    let types = sage_uint(
        node.attribute("DamageTypes").unwrap_or("0"),
        "DamageTypes",
        path,
    )?;
    let amount = finite_number(
        node.attribute("DamageAmount").unwrap_or("0"),
        "DamageAmount",
        path,
    )?;
    data.word(start + 0x24, types);
    data.word(start + 0x28, amount.to_bits());
    data.payload[start + 0x4c] = boolean(node, "InitiallyActive", false, path)?;
    let mut children = significant_children(node).peekable();
    let parent = upgrade_fields(node, KIND, start, data, reference_types, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected FireWeaponWhenDamaged child"));
    }
    let mut entries = vec![base(0xae60_ae98, parent)];
    entries.extend(fields(start, FIELDS));
    Ok((KIND, Table::record(entries)))
}

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const BASE: u32 = 0xae60_ae98;
    const DELAYED: u32 = 0x5a7b_ced3;
    const EXPERIENCE: u32 = 0xe06a_b787;
    const INVISIBILITY: u32 = 0x48c4_38ae;
    const LOCOMOTOR_SET: u32 = 0x2799_dff2;
    const MAX_HEALTH: u32 = 0x7c9a_2737;
    const UNPAUSE_SPECIAL_POWER: u32 = 0x3fec_abbb;

    let path = data.path;
    let name = node.tag_name().name();
    if matches!(name, "CostModifierUpgrade" | "ReplaceSelfUpgrade") {
        return encode_with_lists(node, pointer, data, reference_types);
    }
    let (kind, size, extra): (u32, usize, &[(&str, u32, usize)]) = match name {
        "Upgrade" => (BASE, 0x24, &[]),
        "DelayedUpgrade" => (DELAYED, 0x28, &[("DelayTime", 0x7631_77a4, 0x24)]),
        "ExperienceScalarUpgrade" => (EXPERIENCE, 0x28, &[("AddXPScalar", 0x6458_64f3, 0x24)]),
        "InvisibilityUpgrade" => (
            INVISIBILITY,
            0x28,
            &[("InvisibilityTemplate", 0x914c_a765, 0x24)],
        ),
        "LocomotorSetUpgrade" => (
            LOCOMOTOR_SET,
            0x28,
            &[("KillLocomotorUpgrade", 0x77e3_f4f7, 0x24)],
        ),
        "MaxHealthUpgrade" => (
            MAX_HEALTH,
            0x2c,
            &[
                ("AddMaxHealth", 0x6458_64f3, 0x24),
                ("ChangeType", 0xb8d6_486a, 0x28),
            ],
        ),
        "UnpauseSpecialPowerUpgrade" => (
            UNPAUSE_SPECIAL_POWER,
            0x2c,
            &[
                ("SpecialPowerTemplate", 0xaee8_3888, 0x24),
                ("ObeyRechageOnTrigger", 0x77e3_f4f7, 0x28),
            ],
        ),
        _ => return Err(invalid(path, format!("unexpected upgrade module {name}"))),
    };
    let allowed: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().chain(extra).map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    for &(field, token, offset) in extra {
        let value = node.attribute(field);
        let offset = start + offset;
        match token {
            0x77e3_f4f7 => data.payload[offset] = boolean(node, field, false, path)?,
            0x7631_77a4 => data.word(offset, sage_uint(value.unwrap_or("0"), field, path)?),
            0x6458_64f3 => {
                data.word(
                    offset,
                    finite_number(value.unwrap_or("0"), field, path)?.to_bits(),
                );
            }
            0xb8d6_486a => {
                let variant = match value.unwrap_or("SAME_CURRENTHEALTH") {
                    "SAME_CURRENTHEALTH" => 0,
                    "PRESERVE_RATIO" => 1,
                    "ADD_CURRENT_HEALTH_TOO" => 2,
                    value => return Err(invalid(path, format!("invalid {field} {value:?}"))),
                };
                data.word(offset, variant);
            }
            0x914c_a765 => {
                let ordinal = reference(
                    required(node, field, path)?,
                    "InvisibilityTemplate",
                    path,
                    references,
                    reference_types,
                )?;
                data.word(offset, ordinal);
            }
            0xaee8_3888 => {
                let ordinal = reference(
                    value.unwrap_or(""),
                    "SpecialPowerTemplate",
                    path,
                    references,
                    reference_types,
                )?;
                data.word(offset, ordinal);
            }
            _ => unreachable!("upgrade variant field codec"),
        }
    }
    let mut children = significant_children(node).peekable();
    let inherited = upgrade_fields(node, kind, start, data, reference_types, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(path, format!("unexpected {name} child")));
    }
    if kind == BASE {
        Ok((kind, inherited))
    } else {
        let mut entries = vec![base(BASE, inherited)];
        entries.extend(fields(start, extra));
        Ok((kind, Table::record(entries)))
    }
}

fn encode_with_lists(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const COST: u32 = 0xcdc0_eff7;
    const REPLACE: u32 = 0x53fb_a9b5;
    const UPGRADE_KIND: u32 = 0xae60_ae98;
    const COST_ATTRIBUTES: &[&str] = &[
        "Percentages",
        "IsUpgradeDiscountUpgrade",
        "StartsActive",
        "Slaughter",
        "UILabel",
    ];
    const REPLACE_ATTRIBUTES: &[&str] = &["NewObjectUnpackTime", "Options"];

    let name = node.tag_name().name();
    let (kind, size, attributes) = if name == "CostModifierUpgrade" {
        (COST, 0x44, COST_ATTRIBUTES)
    } else {
        (REPLACE, 0x34, REPLACE_ATTRIBUTES)
    };
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().map(|&(name, _, _)| name))
        .chain(attributes.iter().copied())
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;

    // EA allocates the derived data first, even though inherited elements come first in XML.
    let inherited_count = significant_children(node)
        .take_while(|child| {
            matches!(
                child.tag_name().name(),
                "CustomAnimAndDuration" | "TriggeredBy" | "ConflictsWith"
            )
        })
        .count();
    let references_start = reference_types.len();
    let mut children = significant_children(node).skip(inherited_count).peekable();
    let mut entries = if kind == COST {
        let percentages: Vec<_> = node
            .attribute("Percentages")
            .unwrap_or("")
            .split_ascii_whitespace()
            .map(|raw| finite_number(raw.strip_suffix('%').unwrap_or(raw), "Percentages", path))
            .collect::<io::Result<_>>()?;
        let count = start + 0x24;
        let values = data.list(count, percentages.len(), 4)?;
        let items = percentages.into_iter().enumerate().map(|(index, value)| {
            let offset = values + index * 4;
            data.word(offset, value.to_bits());
            (0, Target::Data(offset))
        });
        let percentages = Target::Table(Table::array(count, items));
        let booleans = [
            ("IsUpgradeDiscountUpgrade", 0x77e3_f4f7, 0x40),
            ("StartsActive", 0x77e3_f4f7, 0x41),
            ("Slaughter", 0x77e3_f4f7, 0x42),
        ];
        for (field, _, offset) in booleans {
            data.payload[start + offset] = boolean(node, field, false, path)?;
        }
        if let Some(label) = node.attribute("UILabel") {
            data.string(start + 0x2c, label)?;
        }
        let filter = match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter"))) {
            Some(filter) => Target::Table(super::parking::encode_filter(
                filter,
                start + 0x34,
                data,
                reference_types,
            )?),
            None => Target::Null,
        };
        let mut entries = vec![Entry::named("Percentages", 0xa539_3e27, percentages)];
        entries.extend(fields(start, &booleans));
        entries.extend([
            Entry::named("UILabel", 0x1a29_33aa, Target::Data(start + 0x2c)),
            Entry::named("ObjectFilter", 0xe46e_a3b2, filter),
        ]);
        entries
    } else {
        const OPTIONS: &[&str] = &[
            "COPY_TARGET_OBJECT",
            "IGNORE_TERRAIN_RESTRICTIONS",
            "COPY_UPGRADES",
            "DISABLE_DURING_UNPACK",
            "CHECK_BUILD_ASSISTANT",
        ];
        let time = node.attribute("NewObjectUnpackTime").unwrap_or("0s");
        data.word(
            start + 0x24,
            sage_time(time, "NewObjectUnpackTime", path)?.to_bits(),
        );
        let options = node
            .attribute("Options")
            .unwrap_or("DISABLE_DURING_UNPACK")
            .split_ascii_whitespace()
            .try_fold(0, |bits, option| {
                super::super::global_ui::enum_value(option, OPTIONS, path)
                    .map(|index| bits | 1 << index)
            })?;
        data.word(start + 0x28, options);
        fields(
            start,
            &[
                ("NewObjectUnpackTime", 0x9986_69e8, 0x24),
                ("Options", 0xd070_8290, 0x28),
            ],
        )
        .collect()
    };

    let (field, token, offset, asset_type) = if kind == COST {
        ("ApplyToUpgrade", 0x26fa_cfc7, 0x38, "UpgradeTemplate")
    } else {
        ("ReplacementTemplate", 0x148e_d75d, 0x2c, "GameObject")
    };
    let mut ids = Vec::new();
    while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, field))) {
        ids.push(weak_reference(child, asset_type, data, reference_types)?);
    }
    let targets = id_list(data, start + offset, ids)?;
    entries.push(Entry::named(field, token, Target::Table(targets)));
    let derived_references = reference_types.len() - references_start;
    let mut inherited_children = significant_children(node).take(inherited_count).peekable();
    let inherited = upgrade_fields(
        node,
        kind,
        start,
        data,
        reference_types,
        &mut inherited_children,
    )?;
    if children.next().is_some() || inherited_children.next().is_some() {
        return Err(invalid(path, format!("unexpected {name} child")));
    }
    // Reference types are hashed in schema order, independent of allocation order.
    reference_types[references_start..].rotate_left(derived_references);
    entries.insert(0, base(UPGRADE_KIND, inherited));
    Ok((kind, Table::record(entries)))
}
