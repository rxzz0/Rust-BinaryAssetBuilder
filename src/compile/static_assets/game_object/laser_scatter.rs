use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::{percentage, sage_time, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::fields;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xfa3f_1c6a;
    const LIFETIME: &str = "LaserLifetime";
    const GROUND_WIDTH: &str = "LaserHitGroundWidthMultiplier";
    const FIELDS: [(&str, u32, usize); 2] =
        [(LIFETIME, 0x9986_69e8, 8), (GROUND_WIDTH, 0x6458_64f3, 0xc)];
    let path = data.path;
    validate_attributes(node, &["id", LIFETIME, GROUND_WIDTH], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected LaserScatterUpdate content"));
    }
    let lifetime = sage_time(node.attribute(LIFETIME).unwrap_or("1s"), LIFETIME, path)?;
    let width = percentage(
        node.attribute(GROUND_WIDTH).unwrap_or("10%"),
        GROUND_WIDTH,
        path,
    )?;
    let start = data.allocate(pointer, 0x10)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.word(start + 8, lifetime.to_bits());
    data.word(start + 0xc, width.to_bits());
    entries.extend(fields(start, &FIELDS));
    Ok((KIND, Table::record(entries)))
}
