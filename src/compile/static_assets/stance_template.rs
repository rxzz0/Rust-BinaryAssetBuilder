use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::global_ui::enum_value;
use super::{Marshaled, significant_children, validate_attributes, write_import};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

const STANCES: &[&str] = &["GUARD", "AGGRESSIVE", "HOLD_POSITION", "HOLD_FIRE"];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let definitions: Vec<_> = significant_children(node).collect();
    if definitions.len() > 32 {
        return Err(invalid(path, "StanceTemplate has more than 32 definitions"));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    let first = data.list(4, definitions.len(), 8)?;
    for (index, definition) in definitions.into_iter().enumerate() {
        if !definition.has_tag_name((SAGE_NS, "StanceDefinition")) {
            return Err(invalid(path, "expected StanceDefinition"));
        }
        validate_leaf(definition, &["Type", "AttributeModifier"], path)?;
        let offset = first + index * 8;
        let stance = enum_value(required(definition, "Type", path)?, STANCES, path)?;
        data.word(offset, stance);
        if let Some(value) = definition.attribute("AttributeModifier") {
            let modifier = data.allocate(offset + 4, 4)?;
            if !value.trim().is_empty() {
                write_import(value, "AttributeModifier", modifier, &mut data, &mut out)?;
            }
        }
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    if !out.imports.is_empty() {
        out.imports.push(u32::MAX);
    }
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}
