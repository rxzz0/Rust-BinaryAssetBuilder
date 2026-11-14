use super::binary::BinaryData;
use super::tokenized::{self, Table};
use super::{Marshaled, finite_number, validate_attributes};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::{io, path::Path};

// Read the fixed token table and default fields from the empty EA fixture.
const BASE: &[u8] = include_bytes!(
    "../../../tests/fixtures/ea-batches/weapon-template-root-assets/94d4d96e.0ce6168e.a529f9d4.226acfbb.asset"
);

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ROOT: usize = 0x514;
    const ATTRS: [(&str, usize, &str); 3] = [
        ("AttackRange", 0, "0"),
        ("MinimumAttackRange", 4, "0"),
        ("WeaponSpeed", 0x28, "99999"),
    ];
    validate_attributes(
        node,
        &[
            "id",
            "AttackRange",
            "MinimumAttackRange",
            "WeaponSpeed",
            "ClipSize",
            "CanFireWhileMoving",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unsupported WeaponTemplate child"));
    }
    let mut data = BinaryData {
        payload: BASE[32 + ROOT..].to_vec(),
        relocations: Vec::new(),
        path,
    };
    let table = Table::from_layout(&BASE[32..32 + ROOT], &data.payload, path)?;
    for (name, offset, default) in ATTRS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    let clip = node
        .attribute("ClipSize")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid WeaponTemplate ClipSize"))?;
    data.word(0x6c, clip as u32);
    data.payload[0x126] = match node
        .attribute("CanFireWhileMoving")
        .unwrap_or("false")
        .trim()
    {
        "true" | "1" => 1,
        "false" | "0" => 0,
        _ => return Err(invalid(path, "invalid CanFireWhileMoving")),
    };
    tokenized::finish(table, data, Marshaled::default())
}
