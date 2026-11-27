use super::binary::BinaryData;
use super::{Marshaled, significant_children, text_value, validate_attributes, write_import};
use crate::compile::{SAGE_NS, hash_source_file, invalid, required};
use roxmltree::Node;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) fn texture_with_resolver(
    node: Node<'_, '_>,
    path: &Path,
    resolve: impl Fn(&str) -> io::Result<PathBuf>,
) -> io::Result<(Marshaled, Vec<u32>)> {
    validate_attributes(node, &["id", "CustomData"], path)?;
    let custom = resolve(required(node, "CustomData", path)?)?;
    let mut checksums = vec![hash_source_file(&custom, io::sink())?];
    let mut data = layout(path, 20);
    data.string(4, &sdk_path(&custom)?)?;
    let files: Vec<_> = significant_children(node).collect();
    let first = data.list(12, files.len(), 8)?;
    for (index, child) in files.into_iter().enumerate() {
        expect(child, "File", path)?;
        let file = resolve(text_value(child, path)?)?;
        let mut bytes = Vec::new();
        checksums.push(hash_source_file(&file, &mut bytes)?);
        let offset = first + index * 8;
        let size = bytes.len();
        let padded = size
            .checked_next_multiple_of(4)
            .ok_or_else(|| invalid(path, "pipeline data is too large"))?;
        let start = data.allocate(offset, padded)?;
        data.word(offset + 4, size as u32);
        data.payload[start..start + size].copy_from_slice(&bytes);
    }
    Ok((data.finish(Marshaled::default()), checksums))
}

pub(super) fn texture_collection(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    super::reference_list(node, path, "Texture", "TestTexture", vec![0])
}

pub(super) fn game_object(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut names = Vec::new();
    let mut modules = None;
    let mut textures = Vec::new();
    for child in significant_children(node) {
        match child.tag_name().name() {
            "DisplayName" if child.has_tag_name((SAGE_NS, "DisplayName")) => names.push(child),
            "Modules" if child.has_tag_name((SAGE_NS, "Modules")) && modules.is_none() => {
                modules = Some(child);
            }
            "TextureCollection" if child.has_tag_name((SAGE_NS, "TextureCollection")) => {
                textures.push(text_value(child, path)?);
            }
            _ => return Err(invalid(path, "unexpected TestGameObject content")),
        }
    }
    let mut data = layout(path, 24);
    let mut result = Marshaled::default();
    let first = data.list(4, names.len(), 8)?;
    for (index, child) in names.into_iter().enumerate() {
        validate_attributes(child, &[], path)?;
        if child.children().any(|part| part.is_element()) {
            return Err(invalid(path, "expected TestGameObject DisplayName text"));
        }
        let name: String = child
            .children()
            .filter(Node::is_text)
            .filter_map(|part| part.text())
            .collect();
        data.string(first + index * 8, &name)?;
    }
    if let Some(modules) = modules {
        validate_attributes(modules, &[], path)?;
        let mut a = Vec::new();
        let mut b = Vec::new();
        for child in significant_children(modules) {
            if child.has_tag_name((SAGE_NS, "TestModuleA")) {
                a.push(child);
            } else if child.has_tag_name((SAGE_NS, "TestModuleB")) {
                b.push(child);
            } else {
                return Err(invalid(path, "unexpected TestModules content"));
            }
        }
        let start = data.allocate(12, 16)?;
        for (offset, children) in [(start, a), (start + 8, b)] {
            let first = data.list(offset, children.len(), 8)?;
            for (index, child) in children.into_iter().enumerate() {
                validate_attributes(child, &["id"], path)?;
                if significant_children(child).next().is_some() {
                    return Err(invalid(path, "unexpected test module content"));
                }
                if let Some(id) = child.attribute("id") {
                    data.string(first + index * 8, id)?;
                }
            }
        }
    }
    let first = data.list(16, textures.len(), 4)?;
    for (index, texture) in textures.into_iter().enumerate() {
        write_import(
            texture,
            "TestTextureCollection",
            first + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    data.relocations.sort_unstable();
    Ok(data.finish(result))
}

fn expect(node: Node<'_, '_>, name: &str, path: &Path) -> io::Result<()> {
    if node.has_tag_name((SAGE_NS, name)) {
        Ok(())
    } else {
        Err(invalid(path, format!("expected pipeline {name} element")))
    }
}

fn layout(path: &Path, size: usize) -> BinaryData<'_> {
    BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    }
}

pub(super) fn resolve_local(document: &Path, value: &str) -> io::Result<PathBuf> {
    let value = value.trim().replace('\\', "/");
    let path = Path::new(&value);
    if value.is_empty() || path.has_root() && !path.is_absolute() {
        return Err(invalid(document, "expected a pipeline source file path"));
    }
    let path = document.with_file_name(path);
    fs::canonicalize(&path).map_err(|error| invalid(&path, error))
}

fn sdk_path(path: &Path) -> io::Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid(path, "pipeline file path must be Unicode"))?;
    Ok(text
        .strip_prefix(r"\\?\UNC\")
        .map(|text| format!(r"\\{text}"))
        .unwrap_or_else(|| text.strip_prefix(r"\\?\").unwrap_or(text).to_owned())
        .replace('/', "\\")
        .to_lowercase())
}
