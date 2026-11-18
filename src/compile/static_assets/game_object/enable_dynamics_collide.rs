use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, validate_attributes};
use super::behaviors_basic::{base, module_base};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xfc7a_9cb5;
const COLLIDE: u32 = 0x70e5_c103;
const FLOAT: u32 = 0x6458_64f3;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id", "Weight"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EnableDynamicsCollide content"));
    }
    let weight = finite_number(node.attribute("Weight").unwrap_or("0.1"), "Weight", path)?;
    let start = data.allocate(pointer, 12)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    data.word(start + 8, weight.to_bits());
    let table = Table::record(vec![
        base(COLLIDE, Table::record(vec![behavior])),
        Entry::named("Weight", FLOAT, Target::Data(start + 8)),
    ]);
    Ok((KIND, table))
}
