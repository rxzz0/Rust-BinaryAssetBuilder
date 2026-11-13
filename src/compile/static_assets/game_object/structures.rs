use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{hash_name, sage_time, validate_attributes};
use crate::compile::{has_content, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Option<(u32, Table)>> {
    let (kind, size, allowed): (u32, usize, &[&str]) = match node.tag_name().name() {
        "BuildingBehavior" => (0x815a_9161, 0x28, &["id", "NightWindowObject"]),
        "StructureUnpackUpdate" => (0x977f_8391, 0x14, &["id", "UnpackTime"]),
        "StructureSellUpdate" => (
            0x4e9c_b811,
            0x10,
            &["id", "SellTime", "ActiveModelCondition"],
        ),
        _ => return Ok(None),
    };
    validate_attributes(node, allowed, data.path)?;
    if has_content(node) {
        return Err(invalid(data.path, "unexpected structural behavior content"));
    }
    let id = required(node, "id", data.path)?;
    let offset = data.allocate(pointer, size)?;
    data.word(offset, kind);
    data.word(offset + 4, hash_name(id));
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(offset)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(offset + 4)),
    ]);
    let mut base = module;
    for kind in [0xd75d_93a2, 0xa50d_729a, 0x2900_211d] {
        base = Table::record(vec![Entry::new(1, kind, Target::Table(base))]);
    }
    let mut entries = vec![Entry::new(1, 0xf100_2505, Target::Table(base))];
    match node.tag_name().name() {
        "BuildingBehavior" => {
            if let Some(value) = node.attribute("NightWindowObject") {
                data.string(offset + 8, value)?;
            }
            for (name, field) in [
                ("NightWindowObject", 8),
                ("FireWindowObject", 0x10),
                ("GlowWindowObject", 0x18),
                ("NightFireWindowObject", 0x20),
            ] {
                entries.push(Entry::named(
                    name,
                    0x805d_25f4,
                    Target::Data(offset + field),
                ));
            }
        }
        "StructureUnpackUpdate" => {
            data.word(
                offset + 8,
                sage_time(
                    node.attribute("UnpackTime").unwrap_or("0s"),
                    "UnpackTime",
                    data.path,
                )?
                .to_bits(),
            );
            data.word(offset + 0x10, 5.0_f32.to_bits());
            entries.extend([
                Entry::named("UnpackTime", 0x9986_69e8, Target::Data(offset + 8)),
                Entry::named("UnpackCompleteSound", 0x63d7_b468, Target::Null),
                Entry::named(
                    "OffsetHeightAboveWater",
                    0x6458_64f3,
                    Target::Data(offset + 0x10),
                ),
            ]);
        }
        "StructureSellUpdate" => {
            data.word(
                offset + 8,
                sage_time(
                    node.attribute("SellTime").unwrap_or("0s"),
                    "SellTime",
                    data.path,
                )?
                .to_bits(),
            );
            if let Some(value) = node.attribute("ActiveModelCondition") {
                if value != "SOLD" {
                    return Err(invalid(data.path, "unsupported ActiveModelCondition"));
                }
                data.word(offset + 0xc, 11);
            }
            entries.extend([
                Entry::named("SellTime", 0x9986_69e8, Target::Data(offset + 8)),
                Entry::named(
                    "ActiveModelCondition",
                    0x1572_3e0b,
                    Target::Data(offset + 0xc),
                ),
            ]);
        }
        _ => unreachable!(),
    }
    Ok(Some((kind, Table::record(entries))))
}
