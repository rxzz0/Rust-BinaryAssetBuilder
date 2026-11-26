use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::validate_attributes;
use super::behaviors_basic::module_base;
use super::secondary_damage::{STATUS_BYTES, write_status_flags};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x5717_d9dc;
    const STATUS: &str = "InvalidStatus";

    let path = data.path;
    validate_attributes(node, &["id", STATUS], path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected InvalidTargetsDeathUpdate content",
        ));
    }

    let start = data.allocate(pointer, 12)?;
    let base = module_base(node, KIND, start, data, true)?;
    let status = match node.attribute(STATUS) {
        Some(value) => {
            let offset = data.allocate(start + 8, STATUS_BYTES)?;
            write_status_flags(value, offset, data)?;
            Target::Data(offset)
        }
        None => Target::Null,
    };
    Ok((
        KIND,
        Table::record(vec![base, Entry::named(STATUS, 0xa7bd_811e, status)]),
    ))
}
