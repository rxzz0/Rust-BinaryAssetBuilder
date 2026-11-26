use super::super::tokenized::Table;
use super::super::{binary::BinaryData, finite_number, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::{fields, write_model_conditions};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xfc22_2f04;
    const FLAGS: &str = "ModelConditionsToSet";
    const SCALARS: [(&str, u32, usize); 3] = [
        ("InitialVelocity", 0x6458_64f3, 0x44),
        ("Acceleration", 0x6458_64f3, 0x48),
        ("TerminalVelocity", 0x6458_64f3, 0x4c),
    ];

    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            FLAGS,
            "InitialVelocity",
            "Acceleration",
            "TerminalVelocity",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected FreefallUpdate content"));
    }

    let start = data.allocate(pointer, 0x50)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    write_model_conditions(node.attribute(FLAGS).unwrap_or(""), start + 8, data)?;
    for ((name, _, offset), default) in SCALARS.into_iter().zip(["0", "1", "15"]) {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    entries.extend(fields(start, &[(FLAGS, 0x67db_bfc9, 8)]));
    entries.extend(fields(start, &SCALARS));
    Ok((KIND, Table::record(entries)))
}
