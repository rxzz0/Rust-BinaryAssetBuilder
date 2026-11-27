use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::{
    AssetReference, finite_number, sage_uint, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, upgrade_fields};
use super::draws::{boolean, fields};
use super::passive_area::reference;
use super::spawn::UPGRADE;
use crate::compile::{invalid, required};
use roxmltree::Node;
use std::io;

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
