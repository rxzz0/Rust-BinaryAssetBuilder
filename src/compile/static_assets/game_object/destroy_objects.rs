use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, required_element, significant_children, validate_attributes};
use super::behaviors_basic::module_base;
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
    const KIND: u32 = 0x7c27_ad44;
    let path = data.path;
    validate_attributes(node, &["id", "Radius"], path)?;
    let mut children = significant_children(node);
    let child = required_element(&mut children, "ObjectFilter", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected DestroyObjectsUpdate child"));
    }
    let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
    let start = data.allocate(pointer, 0x7c)?;
    let parent = module_base(node, KIND, start, data, true)?;
    data.word(start + 8, radius.to_bits());
    let filter = encode_inline_filter(child, start + 0xc, data, reference_types)?;
    Ok((
        KIND,
        Table::record(vec![
            parent,
            Entry::named("Radius", 0x6458_64f3, Target::Data(start + 8)),
            Entry::named("ObjectFilter", 0xe46e_a3b2, Target::Table(filter)),
        ]),
    ))
}
