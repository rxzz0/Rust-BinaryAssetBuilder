use super::super::tokenized::Table;
use super::super::{binary::BinaryData, finite_number, sage_time, validate_attributes};
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
    const KIND: u32 = 0xfb57_ab87;
    const FIELDS: [(&str, u32, usize); 2] =
        [("Opacity", 0x6458_64f3, 8), ("Duration", 0x9986_69e8, 0xc)];

    let path = data.path;
    validate_attributes(node, &["id", "Opacity", "Duration"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected TimedOpacityUpdate content"));
    }

    let start = data.allocate(pointer, 0x10)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let opacity = finite_number(node.attribute("Opacity").unwrap_or("0.5"), "Opacity", path)?;
    let duration = sage_time(node.attribute("Duration").unwrap_or("5s"), "Duration", path)?;
    data.word(start + 8, opacity.to_bits());
    data.word(start + 0xc, duration.to_bits());
    entries.extend(fields(start, &FIELDS));
    Ok((KIND, Table::record(entries)))
}
