use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::boolean;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x1c8b_e51d;
const FX: u32 = 0x0d1c_8e83;
const TIME: u32 = 0x9986_69e8;
const POWER: u32 = 0xaee8_3888;
const SHADER: u32 = 0x8a16_c611;
const FIELDS: &[(&str, u32, usize)] = &[
    ("TriggerFX", FX, 8),
    ("TargetFX", FX, 12),
    ("FadeInTime", TIME, 16),
    ("InitialDelayTime", TIME, 20),
    ("DelayTime", TIME, 24),
    ("ScaleFadeInTimeByDistance", 0x6458_64f3, 28),
    ("GetObjectsCurrentlyAtStorePosition", 0x77e3_f4f7, 0x38),
    ("Telefrag", 0x8044_6a38, 0x20),
    ("SpecialPowerTemplate", POWER, 0x24),
    ("Shader", SHADER, 0x28),
    ("DisabledTypesToProcess", 0x1965_70c9, 0x2c),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = FIELDS
        .iter()
        .map(|&(name, _, _)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x3c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let mut shader = None;
    for attr in node.attributes() {
        let Some(&(_, kind, offset)) = FIELDS
            .iter()
            .find(|&&(name, kind, _)| matches!(kind, FX | POWER | SHADER) && name == attr.name())
        else {
            continue;
        };
        let target_type = match kind {
            FX => "FXList",
            POWER => "SpecialPowerTemplate",
            SHADER => "ShaderOverride",
            _ => unreachable!(),
        };
        let ordinal = super::passive_area::reference(
            attr.value(),
            target_type,
            path,
            references,
            reference_types,
        )?;
        let offset = if kind == SHADER {
            let offset = data.allocate(start + offset, 4)?;
            shader = Some(offset);
            offset
        } else {
            start + offset
        };
        data.word(offset, ordinal);
    }
    use super::super::global_ui::enum_value;
    for &(name, kind, relative) in FIELDS {
        let offset = start + relative;
        let value = node.attribute(name);
        let target = match kind {
            SHADER => shader.map_or(Target::Null, Target::Data),
            _ => {
                match kind {
                    TIME => data.word(
                        offset,
                        sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
                    ),
                    0x6458_64f3 => data.word(
                        offset,
                        finite_number(value.unwrap_or("0"), name, path)?.to_bits(),
                    ),
                    0x77e3_f4f7 => data.payload[offset] = boolean(node, name, false, path)?,
                    0x8044_6a38 => data.word(
                        offset,
                        enum_value(
                            value.unwrap_or("NONE"),
                            &[
                                "NONE",
                                "HEALTH_SUBTRACTION",
                                "INSTANT_DEATH_TELEPORTER_ONLY",
                                "SWAP",
                            ],
                            path,
                        )?,
                    ),
                    0x1965_70c9 => {
                        let mut bits = 0;
                        for flag in value.unwrap_or("HELD").split_ascii_whitespace() {
                            bits |= 1 << enum_value(flag, super::production::DISABLED, path)?;
                        }
                        data.word(offset, bits);
                    }
                    _ => {}
                }
                Target::Data(offset)
            }
        };
        entries.push(Entry::named(name, kind, target));
    }
    let mut children = significant_children(node).peekable();
    for (name, relative) in [
        ("InstaKillObjectFilter", 0x30),
        ("CollidesWithOnTeleport", 0x34),
    ] {
        let target =
            if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                let offset = data.allocate(start + relative, 0x70)?;
                Target::Table(super::parking::encode_inline_filter(
                    child,
                    offset,
                    data,
                    reference_types,
                )?)
            } else {
                Target::Null
            };
        entries.push(Entry::named(name, 0xe46e_a3b2, target));
    }
    if children.next().is_some() {
        return Err(invalid(
            path,
            "unexpected TeleportStoredObjectsUpdate child",
        ));
    }
    Ok((KIND, Table::record(entries)))
}
