use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::validate_attributes;
use super::behaviors_basic::module_base;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x64b1_001a;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id"], path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected SubStructureStorageBehavior content",
        ));
    }
    let start = data.allocate(pointer, 8)?;
    let base = module_base(node, KIND, start, data, false)?;
    Ok((KIND, Table::record(vec![base])))
}
