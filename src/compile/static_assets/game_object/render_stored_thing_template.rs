use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::{has_content, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xb4c2_71d2;
    const CONSUME_ITEM: &str = "ConsumeItem";
    let path = data.path;
    validate_attributes(node, &["id", CONSUME_ITEM], path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected RenderStoredThingTemplate content",
        ));
    }
    let start = data.allocate(pointer, 0xc)?;
    let mut entries = vec![module_base(node, KIND, start, data, false)?];
    data.payload[start + 8] = boolean(node, CONSUME_ITEM, true, path)?;
    entries.extend(fields(start, &[(CONSUME_ITEM, 0x77e3_f4f7, 8)]));
    Ok((KIND, Table::record(entries)))
}
