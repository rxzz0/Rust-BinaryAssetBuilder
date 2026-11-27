use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, hash_name, percentage, sage_time, significant_children,
    validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) const TOKEN_LAYOUT: &[u8; 0x79c] = include_bytes!("active_body_tokens.bin");
pub(super) const SIZE: usize = 0x4c;
pub(super) const SHIELD_SIZE: usize = 0x6c;
pub(super) const SHIELD: u32 = 0xd356_5857;

const SHIELD_FX: [&str; 4] = [
    "ShieldEnabledFX",
    "ShieldDepleteFX",
    "ShieldRechargeEndFX",
    "ShieldTakeDamageFX",
];
pub(super) const KIND: u32 = 0x5a0e_3d9b;
pub(super) const IMMORTAL: u32 = 0x88e9_4f7a;
pub(super) const MANAGED_ACTIVE: u32 = 0xee0d_6e31;

pub(super) fn variant(name: &str) -> Option<(u32, &'static [u32])> {
    Some(match name {
        "ActiveBody" => (KIND, &[]),
        "ShieldBody" => (SHIELD, &[]),
        "ImmortalBody" => (IMMORTAL, &[KIND]),
        "HighlanderBody" => (0x6f0f_9f52, &[KIND]),
        "ManagedActiveBody" => (MANAGED_ACTIVE, &[KIND]),
        "ManagedImmortalBody" => (0xe48b_f32c, &[KIND, MANAGED_ACTIVE]),
        "ManagerBody" => (0x7198_2eac, &[KIND, IMMORTAL]),
        _ => return None,
    })
}

pub(super) fn encode(node: Node<'_, '_>, kind: u32, path: &Path) -> io::Result<[u8; SIZE]> {
    let mut attributes = vec![
        "id",
        "MaxHealth",
        "MaxHealthDamagedPercentage",
        "MaxHealthReallyDamagedPercentage",
        "InitialHealth",
        "DodgePercent",
        "EnteringDamagedTransitionTime",
        "EnteringReallyDamagedTransitionTime",
        "CheerRadius",
        "RemoveUpgradesOnDeath",
        "BurningDeathBehavior",
    ];
    if kind == SHIELD {
        attributes.extend(SHIELD_FX);
        attributes.extend([
            "ShieldAmount",
            "ShieldArmor",
            "ShieldRechargeIdleTime",
            "ShieldActiveModelCondition",
        ]);
    }
    validate_attributes(node, &attributes, path)?;
    let id = required(node, "id", path)?;
    let real = |name, default| finite_number(node.attribute(name).unwrap_or(default), name, path);
    let percent = |name, default| percentage(node.attribute(name).unwrap_or(default), name, path);
    let time = |name| sage_time(node.attribute(name).unwrap_or("0s"), name, path);
    let mut body = [0; SIZE];
    for (offset, value) in [
        (0, kind),
        (4, hash_name(id)),
        (8, real("MaxHealth", "0")?.to_bits()),
        (
            0xc,
            percent("MaxHealthDamagedPercentage", "66.666666")?.to_bits(),
        ),
        (
            0x10,
            percent("MaxHealthReallyDamagedPercentage", "33.333333")?.to_bits(),
        ),
        (0x14, real("InitialHealth", "-1")?.to_bits()),
        (0x18, real("DodgePercent", "0")?.to_bits()),
        (0x1c, time("EnteringDamagedTransitionTime")?.to_bits()),
        (0x20, time("EnteringReallyDamagedTransitionTime")?.to_bits()),
        (0x38, real("CheerRadius", "200")?.to_bits()),
    ] {
        body[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (name, offset) in [
        ("RemoveUpgradesOnDeath", 0x48),
        ("BurningDeathBehavior", 0x49),
    ] {
        body[offset] = match node.attribute(name).map(str::trim) {
            None | Some("false" | "0") => 0,
            Some("true" | "1") => 1,
            Some(_) => return Err(invalid(path, format!("invalid ActiveBody {name}"))),
        };
    }
    Ok(body)
}

pub(super) fn shield(
    node: Node<'_, '_>,
    start: usize,
    inherited: Table,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let mut armor = 0;
    for attribute in node.attributes() {
        if let Some(index) = SHIELD_FX.iter().position(|name| *name == attribute.name()) {
            let reference = super::passive_area::reference(
                attribute.value(),
                "FXList",
                path,
                references,
                reference_types,
            )?;
            data.word(start + SIZE + index * 4, reference);
        } else if attribute.name() == "ShieldArmor" {
            armor = super::passive_area::reference(
                attribute.value(),
                "ArmorTemplate",
                path,
                references,
                reference_types,
            )?;
        }
    }

    let mut entries = vec![super::behaviors_basic::base(KIND, inherited)];
    entries.extend(SHIELD_FX.into_iter().enumerate().map(|(index, name)| {
        Entry::named(name, 0x0d1c_8e83, Target::Data(start + SIZE + index * 4))
    }));
    let amount = start + 0x5c;
    data.word(
        amount,
        finite_number(
            node.attribute("ShieldAmount").unwrap_or("0"),
            "ShieldAmount",
            path,
        )?
        .to_bits(),
    );
    entries.push(Entry::named(
        "ShieldAmount",
        0x6458_64f3,
        Target::Data(amount),
    ));
    let armor_target = if armor == 0 {
        Target::Null
    } else {
        let offset = data.allocate(start + 0x60, 4)?;
        data.word(offset, armor);
        Target::Data(offset)
    };
    entries.push(Entry::named("ShieldArmor", 0x34e9_127b, armor_target));
    let recharge = start + 0x64;
    data.word(
        recharge,
        sage_time(
            node.attribute("ShieldRechargeIdleTime").unwrap_or("0s"),
            "ShieldRechargeIdleTime",
            path,
        )?
        .to_bits(),
    );
    entries.push(Entry::named(
        "ShieldRechargeIdleTime",
        0x9986_69e8,
        Target::Data(recharge),
    ));
    let condition = start + 0x68;
    let flag = node
        .attribute("ShieldActiveModelCondition")
        .unwrap_or("INVALID");
    data.word(
        condition,
        if flag == "INVALID" {
            u32::MAX
        } else {
            super::draws::model_condition_index(flag, path)? as u32
        },
    );
    entries.push(Entry::named(
        "ShieldActiveModelCondition",
        0x1572_3e0b,
        Target::Data(condition),
    ));
    Ok(Table::record(entries))
}

pub(super) fn damage_ranges(
    node: Node<'_, '_>,
    body: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Option<Target>> {
    let path = data.path;
    let ranges: Vec<_> = significant_children(node).collect();
    if ranges.is_empty() {
        return Ok(None);
    }
    let count = body + 0x40;
    let start = data.list(count, ranges.len(), 12)?;
    let mut items = Vec::with_capacity(ranges.len());
    for (index, range) in ranges.into_iter().enumerate() {
        if !range.has_tag_name((SAGE_NS, "DamageRangeModifier")) || has_content(range) {
            return Err(invalid(path, "expected ActiveBody DamageRangeModifier"));
        }
        validate_attributes(range, &["Min", "Max", "AttributeModifier"], path)?;
        let offset = start + index * 12;
        for (slot, name) in [(offset, "Min"), (offset + 4, "Max")] {
            data.word(
                slot,
                percentage(required(range, name, path)?, name, path)?.to_bits(),
            );
        }
        let ordinal = super::passive_area::reference(
            range
                .attribute("AttributeModifier")
                .ok_or_else(|| invalid(path, "DamageRangeModifier requires AttributeModifier"))?,
            "AttributeModifier",
            path,
            references,
            reference_types,
        )?;
        data.word(offset + 8, ordinal);
        items.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Min", 0x6458_64f3, Target::Data(offset)),
                Entry::named("Max", 0x6458_64f3, Target::Data(offset + 4)),
                Entry::named("AttributeModifier", 0x09dd_cff3, Target::Data(offset + 8)),
            ])),
        ));
    }
    Ok(Some(Target::Table(Table::array(count, items))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    #[test]
    fn active_body_matches_ea_oracle() {
        let xml = include_str!("../../../../tests/fixtures/game-object-active-body.xml");
        let oracle = include_bytes!("../../../../tests/fixtures/game-object-active-body.bin");
        let document = Document::parse(xml).unwrap();
        let node = document
            .descendants()
            .find(|item| item.tag_name().name() == "ActiveBody")
            .unwrap();
        assert_eq!(&oracle[4..4 + TOKEN_LAYOUT.len()], TOKEN_LAYOUT);
        let body = encode(node, KIND, Path::new("active.xml")).unwrap();
        assert_eq!(&oracle[oracle.len() - SIZE..], &body);
    }
}
