use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, hash_name, sage_time, significant_children, validate_attributes,
};
use super::draws::boolean;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const DEFAULT_EXIT: u32 = 0x6a65_e73d;
const QUEUE_EXIT: u32 = 0x9063_fb6c;
const QUEUE_FIELDS: &[(&str, u32, usize)] = &[
    ("PlacementViewAngle", 0x4e16_ea4d, 8),
    ("ExitDelaySeconds", 0x9986_69e8, 0xc),
    ("AllowAirborneCreation", 0x77e3_f4f7, 0x2c),
    ("InitialBurst", 0x7631_77a4, 0x10),
    ("NoExitPath", 0x77e3_f4f7, 0x2d),
    ("CanRallyToSlaughter", 0x77e3_f4f7, 0x2e),
    ("ClearAlliesFromDestination", 0x77e3_f4f7, 0x2f),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Option<(u32, Table)>> {
    let (kind, size, allowed): (u32, usize, &[&str]) = match node.tag_name().name() {
        "BuildingBehavior" => (0x815a_9161, 0x28, &["id", "NightWindowObject"]),
        "DefaultProductionExit" => (DEFAULT_EXIT, 0x10, &["id"]),
        "QueueProductionExitUpdate" => (QUEUE_EXIT, 0x30, &[]),
        "StructureUnpackUpdate" => (0x977f_8391, 0x14, &["id", "UnpackTime"]),
        "StructureSellUpdate" => (
            0x4e9c_b811,
            0x10,
            &["id", "SellTime", "ActiveModelCondition"],
        ),
        _ => return Ok(None),
    };
    if kind == QUEUE_EXIT {
        let allowed: Vec<_> = std::iter::once("id")
            .chain(QUEUE_FIELDS.iter().map(|&(name, _, _)| name))
            .collect();
        validate_attributes(node, &allowed, data.path)?;
    } else {
        validate_attributes(node, allowed, data.path)?;
    }
    if !matches!(kind, DEFAULT_EXIT | QUEUE_EXIT) && has_content(node) {
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
        "DefaultProductionExit" | "QueueProductionExitUpdate" => {
            entries.extend(production_exit(node, offset, kind == QUEUE_EXIT, data)?);
        }
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

fn production_exit(
    node: Node<'_, '_>,
    start: usize,
    queue: bool,
    data: &mut BinaryData<'_>,
) -> io::Result<Vec<Entry>> {
    let path = data.path;
    let mut entries = Vec::new();
    if queue {
        for &(name, kind, offset) in QUEUE_FIELDS {
            let value = node.attribute(name);
            let word = match kind {
                0x77e3_f4f7 => {
                    data.payload[start + offset] =
                        boolean(node, name, name == "ClearAlliesFromDestination", path)?;
                    entries.push(Entry::named(name, kind, Target::Data(start + offset)));
                    continue;
                }
                0x7631_77a4 => value
                    .unwrap_or("0")
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?,
                0x9986_69e8 => sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
                _ => {
                    let value = value.unwrap_or("0r").trim();
                    let angle = if let Some(number) = value.strip_suffix('d') {
                        finite_number(number, name, path)?.to_radians()
                    } else if let Some(number) = value.strip_suffix('r') {
                        finite_number(number, name, path)?
                    } else {
                        return Err(invalid(path, format!("invalid {name}")));
                    };
                    angle.to_bits()
                }
            };
            data.word(start + offset, word);
            entries.push(Entry::named(name, kind, Target::Data(start + offset)));
        }
    }
    let mut children = significant_children(node).peekable();
    for (index, name) in ["UnitCreatePoint", "NaturalRallyPoint"]
        .into_iter()
        .enumerate()
    {
        let target = match children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            Some(point) => {
                let offset = if queue {
                    start + 0x14 + index * 12
                } else {
                    data.allocate(start + 8 + index * 4, 12)?
                };
                write_exit_point(point, offset, data)?;
                Target::Data(offset)
            }
            None if queue => {
                return Err(invalid(
                    path,
                    format!("QueueProductionExitUpdate requires {name}"),
                ));
            }
            None => Target::Null,
        };
        entries.push(Entry::named(name, 0x2936_8077, target));
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected production exit child"));
    }
    Ok(entries)
}

fn write_exit_point(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let axes = ["x", "y", "z"];
    validate_attributes(node, &axes, data.path)?;
    if has_content(node) {
        return Err(invalid(
            data.path,
            "unexpected production exit point content",
        ));
    }
    for (i, axis) in axes.into_iter().enumerate() {
        let value = finite_number(node.attribute(axis).unwrap_or("0"), axis, data.path)?;
        data.word(offset + i * 4, value.to_bits());
    }
    Ok(())
}
