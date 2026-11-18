use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::{Marshaled, hash_name, significant_children, validate_attributes, write_import};
use crate::compile::{SAGE_NS, invalid, windows_1252};
use crate::hash::asset_string_hash;
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 8],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(group) = optional_child(node, "ScriptSet", path)? {
        let offset = data.allocate(4, 20)?;
        script_group(group, offset, &mut data, &mut result, 0)?;
    }
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.sort_unstable();
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

fn script_group(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
    depth: usize,
) -> io::Result<()> {
    validate_attributes(node, &["id"], data.path)?;
    if let Some(id) = node.attribute("id") {
        data.word(offset, hash_name(id));
    }
    if depth >= 128 {
        return Err(invalid(
            data.path,
            "script groups nested more than 128 levels",
        ));
    }
    let mut scripts = Vec::new();
    let mut groups = Vec::new();
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "Script")) && groups.is_empty() {
            scripts.push(child);
        } else if child.has_tag_name((SAGE_NS, "ScriptGroup")) {
            groups.push(child);
        } else {
            return Err(invalid(data.path, "expected Script or ScriptGroup"));
        }
    }
    let first = data.list(offset + 4, scripts.len(), 12)?;
    for (index, script) in scripts.into_iter().enumerate() {
        compile_script(script, first + index * 12, data, result)?;
    }
    let first = data.list(offset + 12, groups.len(), 20)?;
    for (index, group) in groups.into_iter().enumerate() {
        script_group(group, first + index * 20, data, result, depth + 1)?;
    }
    Ok(())
}

fn compile_script(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    validate_attributes(node, &["id"], data.path)?;
    if let Some(id) = node.attribute("id") {
        data.word(offset, hash_name(id));
    }
    let actions: Vec<_> = significant_children(node).collect();
    let first = data.list(offset + 4, actions.len(), 12)?;
    for (index, action) in actions.into_iter().enumerate() {
        if !action.has_tag_name((SAGE_NS, "Action")) {
            return Err(invalid(data.path, "expected Action"));
        }
        validate_attributes(action, &["Type"], data.path)?;
        let offset = first + index * 12;
        if let Some(kind) = action.attribute("Type") {
            data.word(offset, script_hash(kind, data.path)?);
        }
        if let Some(parameters) = optional_child(action, "ParameterList", data.path)? {
            parameter_list(parameters, offset + 4, data, result)?;
        }
    }
    Ok(())
}

fn parameter_list(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    validate_attributes(node, &[], data.path)?;
    let parameters: Vec<_> = significant_children(node).collect();
    let first = data.list(offset, parameters.len(), 4)?;
    for (index, parameter) in parameters.into_iter().enumerate() {
        let pointer = first + index * 4;
        if parameter.has_tag_name((SAGE_NS, "ObjectType")) {
            validate_leaf(parameter, &["Object"], data.path)?;
            let offset = data.allocate(pointer, 8)?;
            data.word(offset, 0x0383_bac7);
            object_reference(parameter.attribute("Object"), offset + 4, data, result)?;
        } else if parameter.has_tag_name((SAGE_NS, "ObjectTypeList")) {
            validate_attributes(parameter, &[], data.path)?;
            let offset = data.allocate(pointer, 12)?;
            data.word(offset, 0x1f3c_3128);
            let objects: Vec<_> = significant_children(parameter).collect();
            let first = data.list(offset + 4, objects.len(), 4)?;
            for (index, object) in objects.into_iter().enumerate() {
                if !object.has_tag_name((SAGE_NS, "Object"))
                    || object.children().any(|child| child.is_element())
                {
                    return Err(invalid(data.path, "expected Object reference"));
                }
                validate_attributes(object, &[], data.path)?;
                object_reference(object.text(), first + index * 4, data, result)?;
            }
        } else {
            return Err(invalid(data.path, "expected ObjectType or ObjectTypeList"));
        }
    }
    Ok(())
}

fn object_reference(
    name: Option<&str>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
        write_import(name, "GameObject", offset, data, result)?;
    }
    Ok(())
}

fn script_hash(name: &str, path: &Path) -> io::Result<u32> {
    let bytes = windows_1252(name)
        .ok_or_else(|| invalid(path, "script name contains characters outside Windows-1252"))?;
    Ok(asset_string_hash(&bytes))
}

fn optional_child<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    path: &Path,
) -> io::Result<Option<Node<'a, 'input>>> {
    let mut children = significant_children(node);
    let child = children.next();
    if child.is_some_and(|node| !node.has_tag_name((SAGE_NS, name))) || children.next().is_some() {
        return Err(invalid(path, format!("expected at most one {name}")));
    }
    Ok(child)
}
