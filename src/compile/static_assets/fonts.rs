use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::{
    Marshaled, finite_number, required_element, sage_uint, significant_children, text_value,
    validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn directory(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", "Path"], path)?;
    let data = string_asset(node, 12, "Path", path)?;
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn translation(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", "Name", "TranslateX", "TranslateY"], path)?;
    let mut data = string_asset(node, 20, "Name", path)?;
    for (name, offset) in [("TranslateX", 12), ("TranslateY", 16)] {
        data.word(
            offset,
            finite_number(required(node, name, path)?, name, path)?.to_bits(),
        );
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn settings(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Name"], path)?;
    let mut data = string_asset(node, 20, "Name", path)?;
    let mut children = significant_children(node).peekable();
    if children
        .peek()
        .is_some_and(|child| child.has_tag_name((SAGE_NS, "Size")))
    {
        let size = required_element(&mut children, "Size", path)?;
        data.optional_word(12, sage_uint(text_value(size, path)?, "Size", path)?)?;
    }
    let settings = required_element(&mut children, "Settings", path)?;
    validate_leaf(settings, &["AntiAliased"], path)?;
    required(settings, "AntiAliased", path)?;
    data.payload[16] = boolean(settings, "AntiAliased", false, path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected FontDefaultSetting child"));
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn substitution(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Name"], path)?;
    let mut data = string_asset(node, 20, "Name", path)?;
    let count = significant_children(node).count();
    if !(2..=10).contains(&count) {
        return Err(invalid(
            path,
            "FontSubstitution requires 2 to 10 Substitute elements",
        ));
    }
    let start = data.list(12, count, 20)?;
    let mut children = significant_children(node);
    for offset in (start..start + count * 20).step_by(20) {
        let substitution = required_element(&mut children, "Substitute", path)?;
        validate_attributes(substitution, &["Size"], path)?;
        data.word(offset, size_attribute(substitution, path)?);
        let mut font_children = significant_children(substitution);
        let font = required_element(&mut font_children, "Font", path)?;
        validate_leaf(font, &["Name", "Size", "Bold"], path)?;
        if font_children.next().is_some() {
            return Err(invalid(path, "unexpected Substitute child"));
        }
        data.string(offset + 4, string_attribute(font, "Name", path)?)?;
        data.word(offset + 12, size_attribute(font, path)?);
        data.payload[offset + 16] = boolean(font, "Bold", false, path)?;
    }
    Ok(data.finish(Marshaled::default()))
}

fn string_asset<'a>(
    node: Node<'_, '_>,
    size: usize,
    name: &str,
    path: &'a Path,
) -> io::Result<BinaryData<'a>> {
    let mut data = BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    };
    data.string(4, string_attribute(node, name, path)?)?;
    Ok(data)
}

fn string_attribute<'a>(node: Node<'a, '_>, name: &str, path: &Path) -> io::Result<&'a str> {
    node.attribute(name)
        .ok_or_else(|| invalid(path, format!("missing {name} attribute")))
}

fn size_attribute(node: Node<'_, '_>, path: &Path) -> io::Result<u32> {
    sage_uint(required(node, "Size", path)?, "Size", path)
}
