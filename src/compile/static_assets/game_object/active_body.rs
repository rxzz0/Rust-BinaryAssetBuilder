use super::super::{hash_name, validate_attributes};
use crate::compile::{has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) const TOKEN_LAYOUT: &[u8; 0x79c] = include_bytes!("active_body_tokens.bin");
pub(super) const SIZE: usize = 0x4c;

pub(super) fn encode(node: Node<'_, '_>, path: &Path) -> io::Result<[u8; SIZE]> {
    validate_attributes(node, &["id", "MaxHealth"], path)?;
    if has_content(node) {
        return Err(invalid(path, "ActiveBody cannot contain child elements"));
    }
    let id = required(node, "id", path)?;
    let health = node
        .attribute("MaxHealth")
        .unwrap_or("0")
        .parse::<f32>()
        .map_err(|_| invalid(path, "invalid ActiveBody MaxHealth"))?;
    if !health.is_finite() {
        return Err(invalid(path, "invalid ActiveBody MaxHealth"));
    }
    let mut body = [0; SIZE];
    for (offset, value) in [
        (0, 0x5a0e_3d9b),
        (4, hash_name(id)),
        (8, health.to_bits()),
        (0xc, 0x3f2a_aaaa),
        (0x10, 0x3eaa_aaaa),
        (0x14, 0xbf80_0000),
        (0x38, 0x4348_0000),
    ] {
        body[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    #[test]
    fn active_body_matches_ea_oracle() {
        let xml = include_str!("../../../../tests/fixtures/game-object-active-body.xml");
        let oracle = include_bytes!("../../../../tests/fixtures/game-object-active-body.bin");
        let document = Document::parse(xml).unwrap();
        let node = document
            .descendants()
            .find(|item| item.tag_name().name() == "ActiveBody")
            .unwrap();
        assert_eq!(&oracle[4..4 + TOKEN_LAYOUT.len()], TOKEN_LAYOUT);
        let body = encode(node, Path::new("active.xml")).unwrap();
        assert_eq!(&oracle[oracle.len() - SIZE..], &body);
    }
}
