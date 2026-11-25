use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, required_element, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::parking::encode_inline_filter;
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xcc94_0830;
    let path = data.path;
    validate_attributes(node, &["id", "Radius", "ScanFrequency"], path)?;
    let mut children = significant_children(node);
    let filter = required_element(&mut children, "ObjectFilter", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected SpiderHoleUpdate child"));
    }

    let start = data.allocate(pointer, 0x80)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
    let scan = sage_time(
        node.attribute("ScanFrequency").unwrap_or("1s"),
        "ScanFrequency",
        path,
    )?;
    data.word(start + 8, radius.to_bits());
    data.word(start + 0xc, scan.to_bits());
    entries.extend(fields(
        start,
        &[
            ("Radius", 0x6458_64f3, 8),
            ("ScanFrequency", 0x9986_69e8, 0xc),
        ],
    ));
    let filter = encode_inline_filter(filter, start + 0x10, data, reference_types)?;
    entries.push(Entry::named(
        "ObjectFilter",
        0xe46e_a3b2,
        Target::Table(filter),
    ));
    Ok((KIND, Table::record(entries)))
}
