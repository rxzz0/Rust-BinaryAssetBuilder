use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::{finite_number, has_content, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::fields;
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x6962_45d9;
    const TIME: &str = "RadarExtendTime";
    let path = data.path;
    validate_attributes(node, &["id", TIME], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected RadarUpdate content"));
    }
    let time = finite_number(node.attribute(TIME).unwrap_or("0"), TIME, path)?;
    let start = data.allocate(pointer, 0xc)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.word(start + 8, time.to_bits());
    entries.extend(fields(start, &[(TIME, 0x6458_64f3, 8)]));
    Ok((KIND, Table::record(entries)))
}
