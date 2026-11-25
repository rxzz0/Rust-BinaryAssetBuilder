use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::{finite_number, has_content, sage_uint, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x7517_fe52;
const BOOL_FIELDS: [(&str, u32, usize); 3] = [
    ("TargetHeightIncludesStructures", 0x77e3_f4f7, 0x10),
    ("OnlyWhenMovingDown", 0x77e3_f4f7, 0x11),
    ("SnapToGroundOnDeath", 0x77e3_f4f7, 0x12),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed = [
        "id",
        "TargetHeight",
        "TargetHeightIncludesStructures",
        "OnlyWhenMovingDown",
        "SnapToGroundOnDeath",
        "InitialDelay",
    ];
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in HeightDieUpdate"));
    }

    let start = data.allocate(pointer, 0x14)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let height = finite_number(
        node.attribute("TargetHeight").unwrap_or("0"),
        "TargetHeight",
        path,
    )?;
    data.word(start + 8, height.to_bits());
    let delay_ms = sage_uint(
        node.attribute("InitialDelay").unwrap_or("0"),
        "InitialDelay",
        path,
    )?;
    data.word(start + 0xc, delay_ms.div_ceil(200));
    for (name, _, offset) in BOOL_FIELDS {
        data.payload[start + offset] = boolean(node, name, false, path)?;
    }
    entries.extend(fields(start, &[("TargetHeight", 0x6458_64f3, 8)]));
    entries.extend(fields(start, &BOOL_FIELDS));
    entries.extend(fields(start, &[("InitialDelay", 0x6f6d_1ae7, 0xc)]));
    Ok((KIND, Table::record(entries)))
}
