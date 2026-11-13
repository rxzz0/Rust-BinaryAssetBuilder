use super::super::binary::BinaryData;
use super::super::tokenized::Table;
use super::super::{
    AssetReference, asset_reference, finite_number, sage_time, validate_attributes,
};
use super::draws::{boolean, fields};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const REAL: u32 = 0x6458_64f3;
const TIME: u32 = 0x9986_69e8;
const BOOL: u32 = 0x77e3_f4f7;
const TEXTURE: u32 = 0x4c3e_3938;
const FIELDS: &[(&str, u32, usize)] = &[
    ("Type", 0x3b58_1951, 0),
    ("Texture", TEXTURE, 0x04),
    ("AdditionalTexture", TEXTURE, 0x08),
    ("SizeX", REAL, 0x0c),
    ("SizeY", REAL, 0x10),
    ("OffsetX", REAL, 0x14),
    ("OffsetY", REAL, 0x18),
    ("OpacityStart", REAL, 0x1c),
    ("OpacityFadeInTime", TIME, 0x20),
    ("OpacityPeak", REAL, 0x24),
    ("OpacityFadeOutTime", TIME, 0x28),
    ("OpacityEnd", REAL, 0x2c),
    ("MaxHeight", REAL, 0x30),
    ("OverrideLODVisibility", BOOL, 0x38),
    ("UseHouseColor", BOOL, 0x39),
    ("IsRotatingWithObject", BOOL, 0x3a),
    ("LocalPlayerOnly", BOOL, 0x3b),
    ("SunAngle", REAL, 0x34),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let names: Vec<_> = FIELDS.iter().map(|&(name, _, _)| name).collect();
    validate_attributes(node, &names, path)?;
    if has_content(node) {
        return Err(invalid(path, "ShadowInfo cannot contain content"));
    }
    let kind = node.attribute("Type").unwrap_or("NONE");
    let kind = [
        "NONE",
        "DECAL",
        "ALPHA_DECAL",
        "ADDITIVE_DECAL",
        "ALPHA_DECAL_DYNAMIC",
        "ADDITIVE_DECAL_DYNAMIC",
        "MERGE_DECAL",
    ]
    .iter()
    .position(|&name| name == kind)
    .ok_or_else(|| invalid(path, format!("invalid ShadowInfo Type {kind}")))?;
    let start = data.allocate(pointer, 0x3c)?;
    data.word(start, kind as u32);
    for &(name, field_type, offset) in &FIELDS[1..] {
        let offset = start + offset;
        let value = node.attribute(name);
        match field_type {
            TEXTURE => {
                if let Some(value) = value.filter(|text| !text.trim().is_empty()) {
                    let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                    let id = asset_reference(plain, "Texture", path)?;
                    references.push(id.into());
                    reference_types.push(id.type_id);
                    data.word(offset, references.len() as u32);
                }
            }
            REAL => {
                let default = match name {
                    "OpacityPeak" => "1",
                    "MaxHeight" => "20",
                    _ => "0",
                };
                data.word(
                    offset,
                    finite_number(value.unwrap_or(default), name, path)?.to_bits(),
                );
            }
            TIME => data.word(
                offset,
                sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
            ),
            BOOL => {
                data.payload[offset] = boolean(node, name, name == "IsRotatingWithObject", path)?
            }
            _ => unreachable!(),
        }
    }
    Ok(Table::record(fields(start, FIELDS).collect()))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn shadow_info_matches_ea() {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-shadow.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            document.root_element().first_element_child().unwrap(),
            Path::new("game-object-shadow.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-shadow.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-shadow.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-shadow.manifest"
        ))
        .unwrap();
        assert_eq!(actual.payload.len(), bin.len() - 4);
        assert_eq!(
            actual
                .payload
                .iter()
                .zip(&bin[4..])
                .position(|(a, b)| a != b),
            None
        );
        assert_eq!(actual.relocations, relo[4..]);
        assert!(actual.imports.is_empty());
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.assets[0].references.iter())
        );
    }
}
