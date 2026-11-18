use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::game_object::{encode_render_material, encode_settings_shadow};
use super::{
    Marshaled, finite_number, hash_name, sage_time, sage_uint, significant_children,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn shader_override(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Priority"], path)?;
    let rules: Vec<_> = significant_children(node).collect();
    if rules.is_empty() {
        return Err(invalid(path, "expected at least one shader override Rule"));
    }
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    data.word(
        4,
        sage_uint(node.attribute("Priority").unwrap_or("1"), "Priority", path)?,
    );
    let list = data.list(8, rules.len(), 16)?;
    for (index, rule) in rules.into_iter().enumerate() {
        if !rule.has_tag_name((SAGE_NS, "Rule")) {
            return Err(invalid(path, "expected a shader override Rule"));
        }
        validate_leaf(
            rule,
            &[
                "IfOriginalShaderIs",
                "ReplaceShaderName",
                "ReplaceTechniqueName",
            ],
            path,
        )?;
        let offset = list + index * 16;
        if let Some(shader) = rule.attribute("IfOriginalShaderIs") {
            data.optional_word(offset, hash_name(shader))?;
        }
        data.word(
            offset + 4,
            hash_name(required(rule, "ReplaceShaderName", path)?),
        );
        data.string(
            offset + 8,
            rule.attribute("ReplaceTechniqueName")
                .ok_or_else(|| invalid(path, "missing ReplaceTechniqueName attribute"))?,
        )?;
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn orders(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const FADE: &str = "LineFadeTime";
    const SHADERS: [&str; 2] = ["FormationPreviewShader", "BuildPlacementShader"];
    const LINES: [&str; 6] = [
        "ExecutingLine",
        "PendingLine",
        "PatrolLine",
        "PatrolPendingLine",
        "TargetMoveLine",
        "TargetAttackLine",
    ];
    const NODES: [&str; 4] = ["PatrolNode", "PatrolStart", "PendingNode", "PendingStart"];
    validate_attributes(node, &["id", FADE, SHADERS[0], SHADERS[1]], path)?;
    let mut data = BinaryData {
        payload: vec![0; 128],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.word(
        4,
        sage_time(node.attribute(FADE).unwrap_or("0.25s"), FADE, path)?.to_bits(),
    );
    let mut offsets = [0; 2];
    for (index, name) in SHADERS.into_iter().enumerate() {
        if node.attribute(name).is_some() {
            offsets[index] = data.allocate(8 + index * 4, 4)?;
        }
    }
    for attribute in node.attributes() {
        if let Some(index) = SHADERS.iter().position(|name| *name == attribute.name())
            && !attribute.value().trim().is_empty()
        {
            write_import(
                attribute.value(),
                "ShaderOverride",
                offsets[index],
                &mut data,
                &mut result,
            )?;
        }
    }

    let mut children = significant_children(node).peekable();
    for (index, name) in LINES.into_iter().enumerate() {
        let line = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, name)))
            .ok_or_else(|| invalid(path, format!("expected OrdersManagerSettings {name}")))?;
        write_line(line, 16 + index * 16, &mut data, &mut result)?;
    }
    result.imports.sort_unstable();
    for (index, name) in NODES.into_iter().enumerate() {
        if let Some(shadow) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            encode_settings_shadow(shadow, 112 + index * 4, &mut data, &mut result)?;
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected OrdersManagerSettings element"));
    }
    Ok(data.finish(result))
}

fn write_line(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    validate_leaf(
        node,
        &["Width", "VOffsetRate", "Texture", "TextureTileSize"],
        path,
    )?;
    for (name, default, field) in [
        ("Width", "1.5", 0),
        ("VOffsetRate", "0", 4),
        ("TextureTileSize", "30.0", 12),
    ] {
        data.word(
            offset + field,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    if let Some(texture) = node
        .attribute("Texture")
        .filter(|texture| !texture.trim().is_empty())
    {
        write_import(texture, "Texture", offset + 8, data, result)?;
    }
    Ok(())
}

pub(super) fn connection(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let material = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "FXShader")))
        .filter(|_| children.next().is_none())
        .ok_or_else(|| invalid(path, "expected exactly one ConnectionLineManager FXShader"))?;
    let mut data = BinaryData {
        payload: vec![0; 32],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    encode_render_material(material, 4, &mut data, &mut result)?;
    Ok(data.finish(result))
}
