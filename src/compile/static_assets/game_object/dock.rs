use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, sage_int, sage_time, significant_children, validate_attributes};
use super::behaviors_basic::{base, module_base};
use super::draws::{boolean, fields};
use super::parking::encode_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const DOCK: u32 = 0xd9a4_da61;
const CENTER: u32 = 0xc2f1_ec20;
const WAREHOUSE: u32 = 0xd629_7bd1;
const FILTER: &str = "ForVoiceRetreatThisIsASafeHarborToObjectFilter";
const FLAGS: &[(&str, usize)] = &[
    ("AllowsPassthrough", 24),
    ("GoToRallyPointAfterDock", 25),
    ("ShouldDockInReverse", 26),
];
const TIMES: &[(&str, usize, &str)] = &[
    ("MinDockTime", 12, "0s"),
    ("ObjectsInLineWeight", 16, "13s"),
];
const CENTER_FIELDS: &[(&str, u32, usize)] = &[
    ("ValueMultiplier", 0x6458_64f3, 28),
    ("DistributedDeposit", 0x77e3_f4f7, 32),
];
const WAREHOUSE_FIELDS: &[(&str, u32, usize)] = &[
    ("StartingBoxes", 0x23e6_50b8, 28),
    ("DeleteWhenEmpty", 0x77e3_f4f7, 32),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let name = node.tag_name().name();
    let (kind, size, derived) = match name {
        "DockUpdate" => (DOCK, 28, &[][..]),
        "SupplyCenterDockUpdate" => (CENTER, 36, CENTER_FIELDS),
        "SupplyWarehouseDockUpdate" => (WAREHOUSE, 36, WAREHOUSE_FIELDS),
        _ => return Err(invalid(path, format!("unexpected dock module {name}"))),
    };
    let allowed: Vec<_> = ["id", "NumberApproachPositions"]
        .into_iter()
        .chain(FLAGS.iter().map(|&(name, _)| name))
        .chain(TIMES.iter().map(|&(name, ..)| name))
        .chain(derived.iter().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;

    let start = data.allocate(pointer, size)?;
    let mut entries = vec![module_base(node, kind, start, data, true)?];
    let positions = node.attribute("NumberApproachPositions").unwrap_or("0");
    data.word(
        start + 8,
        sage_int(positions, "NumberApproachPositions", path)?,
    );
    entries.push(Entry::named(
        "NumberApproachPositions",
        0x23e6_50b8,
        Target::Data(start + 8),
    ));
    for &(field, offset) in FLAGS {
        data.payload[start + offset] = boolean(node, field, false, path)?;
        entries.push(Entry::named(
            field,
            0x77e3_f4f7,
            Target::Data(start + offset),
        ));
    }
    for &(field, offset, default) in TIMES {
        let value = sage_time(node.attribute(field).unwrap_or(default), field, path)?;
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(
            field,
            0x9986_69e8,
            Target::Data(start + offset),
        ));
    }
    let mut children = significant_children(node);
    let filter = match children.next() {
        Some(child) if child.has_tag_name((SAGE_NS, FILTER)) && children.next().is_none() => {
            Target::Table(encode_filter(child, start + 20, data, reference_types)?)
        }
        None => Target::Null,
        _ => return Err(invalid(path, format!("unexpected {name} child"))),
    };
    entries.push(Entry::named(FILTER, 0xe46e_a3b2, filter));
    let dock = Table::record(entries);
    if kind == DOCK {
        return Ok((kind, dock));
    }
    if kind == CENTER {
        let multiplier = node.attribute("ValueMultiplier").unwrap_or("1.0");
        data.word(
            start + 28,
            finite_number(multiplier, "ValueMultiplier", path)?.to_bits(),
        );
        data.payload[start + 32] = boolean(node, "DistributedDeposit", false, path)?;
    } else {
        let boxes = node.attribute("StartingBoxes").unwrap_or("1");
        data.word(start + 28, sage_int(boxes, "StartingBoxes", path)?);
        data.payload[start + 32] = boolean(node, "DeleteWhenEmpty", false, path)?;
    }
    let mut entries = vec![base(DOCK, dock)];
    entries.extend(fields(start, derived));
    Ok((kind, Table::record(entries)))
}
