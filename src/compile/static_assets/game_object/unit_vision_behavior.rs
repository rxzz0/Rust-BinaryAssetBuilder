use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{has_content, sage_int, validate_attributes};
use super::behaviors_basic::module_base;
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xf29a_36d0;
    let path = data.path;
    validate_attributes(node, &["id", "UpdateRate"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected UnitVisionBehavior content"));
    }

    let start = data.allocate(pointer, 12)?;
    let base = module_base(node, KIND, start, data, false)?;
    let rate = sage_int(
        node.attribute("UpdateRate").unwrap_or("1"),
        "UpdateRate",
        path,
    )?;
    data.word(start + 8, rate);
    Ok((
        KIND,
        Table::record(vec![
            base,
            Entry::named("UpdateRate", 0x23e6_50b8, Target::Data(start + 8)),
        ]),
    ))
}
