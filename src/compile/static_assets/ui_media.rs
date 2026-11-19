use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::{
    Marshaled, sage_int, significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn cursor(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const DIRECTIONS: &str = "NumDirections";
    validate_leaf(node, &["id", "File", DIRECTIONS], path)?;
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    if let Some(file) = node.attribute("File") {
        data.string(4, file)?;
    }
    let directions = sage_int(node.attribute(DIRECTIONS).unwrap_or("1"), DIRECTIONS, path)?;
    data.word(12, directions);
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn basic_cursors(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const NAMES: &[&str] = &["id", "Normal", "Arrow", "Invalid", "EnterText"];
    validate_leaf(node, NAMES, path)?;
    let mut data = BinaryData {
        payload: vec![0; 20],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for attribute in node.attributes() {
        if let Some(index) = NAMES[1..].iter().position(|&name| name == attribute.name()) {
            write_import(
                attribute.value(),
                "MouseCursor",
                4 + index * 4,
                &mut data,
                &mut result,
            )?;
        }
    }
    Ok(data.finish(result))
}

pub(super) fn image_sequence(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let frames: Vec<_> = significant_children(node).collect();
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let start = data.list(4, frames.len(), 4)?;
    for (index, frame) in frames.into_iter().enumerate() {
        if !frame.has_tag_name((SAGE_NS, "Image")) {
            return Err(invalid(path, "expected an ImageSequence Image element"));
        }
        write_import(
            text_value(frame, path)?,
            "PackedTextureImage",
            start + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    Ok(data.finish(result))
}
