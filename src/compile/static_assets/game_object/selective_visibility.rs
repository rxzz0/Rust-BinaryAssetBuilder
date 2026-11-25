use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{sage_time, validate_attributes};
use super::behaviors_basic::module_base;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x6154_a1a4;
    let path = data.path;
    validate_attributes(node, &["id", "UpdateRate", "AllowedPlayers"], path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected SelectiveVisibilityBehavior content",
        ));
    }
    let start = data.allocate(pointer, 0x10)?;
    let inherited = module_base(node, KIND, start, data, false)?;
    let rate = if let Some(value) = node.attribute("UpdateRate") {
        let seconds = sage_time(value, "UpdateRate", path)?;
        let offset = data.allocate(start + 8, 4)?;
        data.word(offset, seconds.to_bits());
        Target::Data(offset)
    } else {
        Target::Null
    };
    let allowed = if let Some(value) = node.attribute("AllowedPlayers") {
        let mut flags = 0;
        for relationship in value.split_ascii_whitespace() {
            flags |= match relationship {
                "ENEMIES" => 1,
                "NEUTRAL" => 2,
                "ALLIES" => 4,
                _ => {
                    return Err(invalid(
                        path,
                        format!("invalid AllowedPlayers value {relationship}"),
                    ));
                }
            };
        }
        let offset = data.allocate(start + 0xc, 4)?;
        data.word(offset, flags);
        Target::Data(offset)
    } else {
        Target::Null
    };
    Ok((
        KIND,
        Table::record(vec![
            inherited,
            Entry::named("UpdateRate", 0x9986_69e8, rate),
            Entry::named("AllowedPlayers", 0x7105_fef3, allowed),
        ]),
    ))
}
