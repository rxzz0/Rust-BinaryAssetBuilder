use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::Table;
use super::super::validate_attributes;
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::secondary_damage::{STATUS_BYTES, write_status_flags};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xf145_a667;
const FIELDS: &[(&str, u32, usize)] = &[
    ("Options", 0x07e1_b584, 8),
    ("RequiredMasterObjectStatus", 0xa7bd_811e, 12),
];
const OPTIONS: &[&str] = &[
    "MASTER",
    "KILL_WHEN_RELEASED",
    "RELEASE_WHEN_MASTER_CHANGES_TARGET",
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id", "Options", "RequiredMasterObjectStatus"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected OCLMonitorUpdate content"));
    }

    let options = node
        .attribute("Options")
        .unwrap_or_default()
        .split_ascii_whitespace()
        .try_fold(0, |bits, option| {
            enum_value(option, OPTIONS, path).map(|index| bits | (1 << index))
        })?;
    let start = data.allocate(pointer, 12 + STATUS_BYTES)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.word(start + 8, options);
    write_status_flags(
        node.attribute("RequiredMasterObjectStatus")
            .unwrap_or_default(),
        start + 12,
        data,
    )?;
    entries.extend(fields(start, FIELDS));
    Ok((KIND, Table::record(entries)))
}
