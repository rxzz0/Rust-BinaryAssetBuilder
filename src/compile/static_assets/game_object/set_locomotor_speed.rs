use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, sage_angle, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::parking::encode_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x2f45_36a8;
const FIELDS: [(&str, u32, usize); 7] = [
    ("Type", 0x8cbf_dd28, 8),
    ("LocomotorSpeedToSet", 0x6458_64f3, 0xc),
    ("LocomotorDeltaPerUpdate", 0x6458_64f3, 0x10),
    ("OtherScaredScanRangeMultiplier", 0x6458_64f3, 0x14),
    ("ObjectFilterRange", 0x6458_64f3, 0x18),
    ("UpdatePeriod", 0x9986_69e8, 0x1c),
    ("VisionArc", 0x4e16_ea4d, 0x20),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x28)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let kind = enum_value(
        node.attribute("Type").unwrap_or("INVALID"),
        &["INVALID", "IN_RANGE_OBJECT_FILTER"],
        path,
    )?;
    data.word(start + 8, kind);
    for &(name, _, offset) in &FIELDS[1..5] {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let period = sage_time(
        node.attribute("UpdatePeriod").unwrap_or("1s"),
        "UpdatePeriod",
        path,
    )?;
    data.word(start + 0x1c, period.to_bits());
    let arc = sage_angle(
        node.attribute("VisionArc").unwrap_or("180d"),
        "VisionArc",
        path,
    )?;
    data.word(start + 0x20, arc.to_bits());
    entries.extend(fields(start, &FIELDS));
    let mut children = significant_children(node);
    let filter = match children.next() {
        Some(child)
            if child.has_tag_name((SAGE_NS, "ObjectFilter")) && children.next().is_none() =>
        {
            Target::Table(encode_filter(child, start + 0x24, data, reference_types)?)
        }
        None => Target::Null,
        _ => return Err(invalid(path, "unexpected SetLocomotorSpeedUpdate child")),
    };
    entries.push(Entry::named("ObjectFilter", 0xe46e_a3b2, filter));
    Ok((KIND, Table::record(entries)))
}
