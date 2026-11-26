use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, validate_attributes};
use super::behaviors_basic::module_base;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xe42d_5618;
    const REAL: u32 = 0x6458_64f3;
    const FIELDS: [(&str, &str, usize); 4] = [
        ("AimLowThreshold", "-0.4", 8),
        ("AimHighThreshold", "0.4", 12),
        ("AimNearDistance", "0", 16),
        ("AimFarDistance", "0", 20),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected AimWeapon content"));
    }

    let start = data.allocate(pointer, 24)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, default, offset) in FIELDS {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(name, REAL, Target::Data(start + offset)));
    }
    Ok((KIND, Table::record(entries)))
}
