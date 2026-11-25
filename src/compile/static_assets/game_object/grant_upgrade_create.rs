use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, validate_attributes};
use super::behaviors_basic::{base, module_base};
use super::draws::boolean;
use super::passive_area::reference;
use super::secondary_damage::write_status_flags;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x04b8_c430;
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "UpgradeToGrant",
            "ExemptStatus",
            "GiveOnBuildComplete",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected GrantUpgradeCreate content"));
    }

    let start = data.allocate(pointer, 0x2c)?;
    let parent = base(
        0x30a3_6b8d,
        Table::record(vec![module_base(node, KIND, start, data, false)?]),
    );
    let upgrade = reference(
        node.attribute("UpgradeToGrant").unwrap_or(""),
        "UpgradeTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 8, upgrade);
    write_status_flags(
        node.attribute("ExemptStatus").unwrap_or(""),
        start + 0xc,
        data,
    )?;
    data.payload[start + 0x28] = boolean(node, "GiveOnBuildComplete", false, path)?;
    Ok((
        KIND,
        Table::record(vec![
            parent,
            Entry::named("UpgradeToGrant", 0x4aba_1f29, Target::Data(start + 8)),
            Entry::named("ExemptStatus", 0xa7bd_811e, Target::Data(start + 0xc)),
            Entry::named(
                "GiveOnBuildComplete",
                0x77e3_f4f7,
                Target::Data(start + 0x28),
            ),
        ]),
    ))
}
