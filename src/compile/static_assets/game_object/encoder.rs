//! Native binary encoding for the first verified GameObject subset.
//!
//! The token layout is the fixed RA3 GameObject field table. Unsupported
//! fields are rejected until their binary representation is verified.

use super::super::{
    Marshaled, asset_reference, fast_hash, hash_name, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required, windows_1252};
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
    validate_attributes(
        node,
        &["id", "Side", "EditorName", "EditorSorting", "KindOf"],
        path,
    )?;
    let mut children = significant_children(node);
    let body = children
        .next()
        .ok_or_else(|| invalid(path, "GameObject requires Body"))?;
    if !body.has_tag_name((SAGE_NS, "Body"))
        || children.next().is_some()
        || body.attributes().len() != 0
    {
        return Err(invalid(path, "only a single GameObject Body is supported"));
    }
    let mut modules = significant_children(body);
    let module = modules
        .next()
        .ok_or_else(|| invalid(path, "Body requires ImmortalBody"))?;
    if !module.has_tag_name((SAGE_NS, "ImmortalBody"))
        || modules.next().is_some()
        || has_content(module)
    {
        return Err(invalid(path, "only ImmortalBody is supported"));
    }
    validate_attributes(module, &["id", "MaxHealth"], path)?;
    let module_id = required(module, "id", path)?;
    let health = module
        .attribute("MaxHealth")
        .unwrap_or("0")
        .parse::<f32>()
        .map_err(|_| invalid(path, "invalid MaxHealth"))?;
    if !health.is_finite() {
        return Err(invalid(path, "invalid MaxHealth"));
    }

    let mut payload = TOKENS.to_vec();
    payload.resize(TOKENS.len() + ROOT_SIZE, 0);
    for &(offset, value) in ROOT_DEFAULTS {
        word(&mut payload, TOKENS.len() + offset, value);
    }
    let root = TOKENS.len();
    let mut reference_types = Vec::new();
    if let Some(kind) = node.attribute("KindOf") {
        for flag in kind.split_ascii_whitespace() {
            let (offset, bit) = match flag {
                "IMMOBILE" => (0, 4),
                "INERT" => (8, 25),
                _ => return Err(invalid(path, format!("unsupported KindOf flag {flag}"))),
            };
            let at = root + offset;
            let value = u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
            word(&mut payload, at, value | (1 << bit));
        }
    }
    if let Some(side) = node.attribute("Side") {
        word(
            &mut payload,
            root + 0x70,
            asset_reference(side, "PlayerTemplate", path)?.instance_id,
        );
        reference_types.push(fast_hash(b"PlayerTemplate"));
    }
    match node.attribute("EditorSorting").unwrap_or("NONE") {
        "NONE" => {}
        "AUDIO" => word(&mut payload, root + 0x7c, 8),
        value => return Err(invalid(path, format!("unsupported EditorSorting {value}"))),
    }
    let mut relocations = Vec::new();
    if let Some(name) = node.attribute("EditorName") {
        let encoded = windows_1252(name)
            .ok_or_else(|| invalid(path, "EditorName contains characters outside Windows-1252"))?;
        let size = (encoded.len() + 1).next_multiple_of(4);
        word(&mut payload, root + 0x74, encoded.len() as u32);
        let start = payload.len() as u32;
        word(&mut payload, root + 0x78, start);
        relocations.push((root + 0x78) as u32);
        payload.extend_from_slice(&encoded);
        payload.resize(payload.len() + size - encoded.len(), 0);
    }
    let body_start = payload.len();
    let body_shift = body_start - (TOKENS.len() + ROOT_SIZE);
    if body_shift != 0 {
        for entry in payload[..TOKENS.len() - 4].as_chunks_mut::<12>().0 {
            let target = u32::from_le_bytes(entry[8..12].try_into().unwrap());
            if target >= (TOKENS.len() + ROOT_SIZE) as u32 {
                entry[8..12].copy_from_slice(&(target + body_shift as u32).to_le_bytes());
            }
        }
    }
    word(&mut payload, root + 0x1dc, body_start as u32);
    relocations.push((root + 0x1dc) as u32);
    payload.resize(body_start + BODY_SIZE, 0);
    for &(offset, value) in BODY_DEFAULTS {
        word(&mut payload, body_start + offset, value);
    }
    word(&mut payload, body_start + 4, hash_name(module_id));
    word(&mut payload, body_start + 8, health.to_bits());
    relocations.push(u32::MAX);
    Ok(Marshaled {
        payload,
        relocations,
        reference_types,
        tokenized: true,
        ..Marshaled::default()
    })
}
