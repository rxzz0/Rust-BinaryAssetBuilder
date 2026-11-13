//! Native binary encoding for the first verified GameObject subset.
//!
//! The token layout is the fixed RA3 GameObject field table. Unsupported
//! fields are rejected until their binary representation is verified.

use super::super::binary::BinaryData;
use super::super::tokenized::{self, Entry, Table, Target};
use super::super::{Marshaled, hash_name, significant_children, validate_attributes};
use super::{
    active_body, ai_update, behaviors_basic, children, client_behaviors, draws, fields, physics,
    structures,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

const TOKENS: &[u8; 0x7b4] = include_bytes!("token_layout.bin");
const ROOT_SIZE: usize = 0x24c;
const BODY_SIZE: usize = 0x4c;

const ROOT_DEFAULTS: &[(usize, u32)] = &[
    (0x4c, 0xffff_ffff),
    (0xa8, 0xbf80_0000),
    (0xb4, 0x4416_0000),
    (0xb8, 0x3f80_0000),
    (0xbc, 0x4120_0000),
    (0xc4, 1),
    (0xc8, 1),
    (0xd8, 0x3f80_0000),
    (0xe8, 0x96),
    (0xec, 0x40a0_0000),
    (0xf0, 0x40a0_0000),
    (0xf4, 0x41a0_0000),
    (0xf8, 0x4040_0000),
    (0xfc, 0x4316_0000),
    (0x100, 0x1e),
    (0x104, 0x3f80_0000),
    (0x10c, 0xbf80_0000),
    (0x11c, 0x43b4_0000),
    (0x120, 1),
    (0x124, 0x7fff_ffff),
    (0x128, 0x19),
    (0x12c, 0xffff_ffff),
    (0x158, 0xffff_ffff),
    (0x15c, 0xffff_ffff),
    (0x160, 0x4000_0000),
    (0x168, 0x3f00_0000),
    (0x16c, 0x3ecc_cccd),
    (0x170, 0x3f66_6666),
    (0x178, 0x1f4),
    (0x17c, 0x4348_0000),
    (0x180, 0x9b),
    (0x240, 0x100),
    (0x248, 0x10100),
];

const BODY_DEFAULTS: &[(usize, u32)] = &[
    (0, 0x88e9_4f7a),
    (0xc, 0x3f2a_aaaa),
    (0x10, 0x3eaa_aaaa),
    (0x14, 0xbf80_0000),
    (0x38, 0x4348_0000),
];

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(in crate::compile::static_assets) fn compile(
    node: Node<'_, '_>,
    path: &Path,
) -> io::Result<Marshaled> {
    let mut body = None;
    let mut behaviors = None;
    let mut draws = None;
    let mut ai = None;
    let mut client = None;
    for child in significant_children(node) {
        let slot = match child.tag_name().name() {
            "Body" => &mut body,
            "Behaviors" => &mut behaviors,
            "Draws" => &mut draws,
            "AI" => &mut ai,
            "ClientBehaviors" => &mut client,
            "DisplayName"
            | "GameDependency"
            | "ObjectResourceInfo"
            | "ArmorSet"
            | "SkirmishAIInformation"
            | "VisionInfo"
            | "Geometry"
                if child.tag_name().namespace() == Some(SAGE_NS) =>
            {
                continue;
            }
            kind => {
                return Err(invalid(
                    path,
                    format!("unsupported GameObject child {kind}"),
                ));
            }
        };
        if child.tag_name().namespace() != Some(SAGE_NS)
            || child.attributes().len() != 0
            || slot.replace(child).is_some()
        {
            return Err(invalid(path, "invalid or repeated GameObject module group"));
        }
    }
    let body = body.ok_or_else(|| invalid(path, "GameObject requires Body"))?;
    let mut modules = significant_children(body);
    let module = modules
        .next()
        .ok_or_else(|| invalid(path, "Body requires ImmortalBody"))?;
    if modules.next().is_some() || module.tag_name().namespace() != Some(SAGE_NS) {
        return Err(invalid(path, "Body requires a single supported module"));
    }
    let (tokens, body_bytes): (&[u8], [u8; BODY_SIZE]) = match module.tag_name().name() {
        "ActiveBody" => (
            active_body::TOKEN_LAYOUT,
            active_body::encode(module, path)?,
        ),
        "ImmortalBody" => (TOKENS, immortal_body(module, path)?),
        kind => return Err(invalid(path, format!("unsupported Body module {kind}"))),
    };

    let mut data = BinaryData {
        payload: vec![0; ROOT_SIZE],
        relocations: Vec::new(),
        path,
    };
    for &(offset, value) in ROOT_DEFAULTS {
        data.word(offset, value);
    }
    let mut template_data = data.payload.clone();
    template_data.extend_from_slice(&body_bytes);
    let mut table = Table::from_layout(tokens, &template_data, path)?;
    let mut reference_types = Vec::new();
    let mut references = Vec::new();
    fields::encode(node, &mut data, &mut reference_types, &mut references)?;
    let mut child_targets =
        children::encode(node, &mut data, &mut references, &mut reference_types)?;
    if let Some(group) = draws {
        let target = draws::encode(group, &mut data, &mut references, &mut reference_types)?;
        child_targets.push(("Draws", target));
    }
    let behavior_tokens = if let Some(group) = behaviors {
        let modules: Vec<_> = significant_children(group).collect();
        let list = data.allocate(0x1d0, 8)?;
        let pointers = data.list(list, modules.len(), 4)?;
        let mut items = Vec::new();
        for (index, module) in modules.into_iter().enumerate() {
            if module.tag_name().namespace() != Some(SAGE_NS) {
                return Err(invalid(path, "unexpected behavior namespace"));
            }
            let encoded = behaviors_basic::encode(
                module,
                pointers + index * 4,
                &mut data,
                &mut references,
                &mut reference_types,
            )?;
            let (kind, fields) = match encoded {
                Some(encoded) => encoded,
                None if module.has_tag_name((SAGE_NS, "Physics")) => {
                    physics::encode(module, pointers + index * 4, &mut data)?
                }
                None => structures::encode(module, pointers + index * 4, &mut data)?.ok_or_else(
                    || {
                        invalid(
                            path,
                            format!("unsupported behavior {}", module.tag_name().name()),
                        )
                    },
                )?,
            };
            items.push((
                0,
                Target::Table(Table::sequence(vec![Entry::new(
                    2,
                    kind,
                    Target::Table(fields),
                )])),
            ));
        }
        Some(Target::Table(Table::array(list, items)))
    } else {
        None
    };
    if let Some(group) = ai {
        let target = ai_update::encode(group, &mut data, &mut references, &mut reference_types)?;
        child_targets.push(("AI", target));
    }
    let body_shift = data.payload.len() - ROOT_SIZE;
    table.shift_data_from(ROOT_SIZE, body_shift);
    let body_start = data.allocate(0x1dc, BODY_SIZE)?;
    data.payload[body_start..body_start + BODY_SIZE].copy_from_slice(&body_bytes);
    if let Some(group) = client {
        let target =
            client_behaviors::encode(group, &mut data, &mut references, &mut reference_types)?;
        child_targets.push(("ClientBehaviors", target));
    }
    if let Some(target) = children::encode_geometry(node, &mut data)? {
        child_targets.push(("Geometry", target));
    }
    if let Some(target) = children::encode_vision(node, &mut data)? {
        child_targets.push(("VisionInfo", target));
    }
    if let Some(target) = behavior_tokens
        && !table.replace_target("Behaviors", target)
    {
        return Err(invalid(path, "GameObject layout is missing Behaviors"));
    }
    for (name, target) in child_targets {
        if !table.replace_target(name, target) {
            return Err(invalid(
                path,
                format!("GameObject layout is missing {name}"),
            ));
        }
    }
    data.relocations.sort_unstable();
    tokenized::finish(
        table,
        data,
        Marshaled {
            reference_types,
            references,
            ..Marshaled::default()
        },
    )
}

fn immortal_body(module: Node<'_, '_>, path: &Path) -> io::Result<[u8; BODY_SIZE]> {
    validate_attributes(module, &["id", "MaxHealth"], path)?;
    if has_content(module) {
        return Err(invalid(path, "ImmortalBody cannot contain content"));
    }
    let module_id = required(module, "id", path)?;
    let health = module
        .attribute("MaxHealth")
        .unwrap_or("0")
        .parse::<f32>()
        .map_err(|_| invalid(path, "invalid MaxHealth"))?;
    if !health.is_finite() {
        return Err(invalid(path, "invalid MaxHealth"));
    }
    let mut body = [0; BODY_SIZE];
    for &(offset, value) in BODY_DEFAULTS {
        word(&mut body, offset, value);
    }
    word(&mut body, 4, hash_name(module_id));
    word(&mut body, 8, health.to_bits());
    Ok(body)
}
