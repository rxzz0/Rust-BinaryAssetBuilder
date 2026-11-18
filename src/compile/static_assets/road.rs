use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::{Marshaled, finite_number, write_import};
use std::{io, path::Path};

pub(super) fn compile(node: roxmltree::Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(
        node,
        &[
            "id",
            "Texture",
            "NormalTexture",
            "RoadWidth",
            "RoadWidthInTexture",
            "Priority",
        ],
        path,
    )?;
    let mut data = BinaryData {
        payload: vec![0; 24],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for (attribute, offset) in [("Texture", 4), ("NormalTexture", 8)] {
        if let Some(name) = node
            .attribute(attribute)
            .filter(|name| !name.trim().is_empty())
        {
            write_import(name, "Texture", offset, &mut data, &mut result)?;
        }
    }
    for (attribute, offset) in [
        ("RoadWidth", 12),
        ("RoadWidthInTexture", 16),
        ("Priority", 20),
    ] {
        if let Some(value) = node.attribute(attribute) {
            data.word(offset, finite_number(value, attribute, path)?.to_bits());
        }
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    Ok(result)
}
