use super::super::binary::BinaryData;
use super::super::{AssetReference, asset_reference, fast_hash};
use crate::compile::{XML_NS, invalid};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io;
use std::sync::OnceLock;

pub(super) fn encode(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
    _references: &mut Vec<AssetReference>,
) -> io::Result<()> {
    const ENCODED: &[&str] = &[
        "id",
        "KindOf",
        "Description",
        "TypeDescription",
        "RadarPriority",
        "PlacementViewAngle",
        "Side",
        "EditorName",
        "EditorSorting",
        "BuildTime",
        "EnergyProduction",
        "CommandSet",
        "SelectPortrait",
        "ButtonImage",
        "CampnessValue",
        "CampnessValueRadius",
        "HealthBoxHeightOffset",
        "ProductionQueueType",
        "BuildPlacementTypeFlag",
        "UnitCategory",
        "WeaponCategory",
    ];
    for attribute in node.attributes() {
        if attribute.namespace() == Some(XML_NS) && attribute.name() == "base" {
            continue;
        }
        if attribute.namespace().is_none()
            && (ENCODED.contains(&attribute.name())
                || schema_defaults()
                    .get(attribute.name())
                    .is_some_and(|default| default == attribute.value()))
        {
            continue;
        }
        return Err(invalid(
            data.path,
            format!(
                "unsupported GameObject attribute {}={:?}",
                attribute.name(),
                attribute.value()
            ),
        ));
    }
    if let Some(flags) = node.attribute("KindOf") {
        write_kind_of(flags, 0, data)?;
    }
    for (name, offset) in [("Description", 0x2c), ("TypeDescription", 0x3c)] {
        if let Some(value) = node.attribute(name) {
            data.string(offset, value)?;
        }
    }
    if let Some(value) = node.attribute("RadarPriority") {
        data.word(
            0x4c,
            enum_value(value, &["STRUCTURE"], "RadarPriority", data.path)?,
        );
    }
    if let Some(value) = node.attribute("PlacementViewAngle") {
        let (number, degrees) = if let Some(number) = value.strip_suffix('d') {
            (number, true)
        } else if let Some(number) = value.strip_suffix('r') {
            (number, false)
        } else {
            return Err(invalid(data.path, "invalid PlacementViewAngle"));
        };
        let parsed = number
            .parse::<f64>()
            .map_err(|_| invalid(data.path, "invalid PlacementViewAngle"))?;
        if !parsed.is_finite() {
            return Err(invalid(data.path, "invalid PlacementViewAngle"));
        }
        let radians = if degrees { parsed.to_radians() } else { parsed };
        data.word(0x60, (radians as f32).to_bits());
    }
    if let Some(value) = node.attribute("Side") {
        data.word(0x70, weak_id(value, "PlayerTemplate", data.path)?);
        reference_types.push(fast_hash(b"PlayerTemplate"));
    }
    if let Some(value) = node.attribute("EditorName") {
        data.string(0x74, value)?;
    }
    if let Some(value) = node.attribute("EditorSorting") {
        let number = match value {
            "NONE" => 0,
            "STRUCTURE" => 1,
            "AUDIO" => 8,
            _ => return Err(invalid(data.path, format!("invalid EditorSorting {value}"))),
        };
        data.word(0x7c, number);
    }
    if let Some(value) = node.attribute("BuildTime") {
        data.word(0x84, finite_float(value, "BuildTime", data.path)?.to_bits());
    }
    if let Some(value) = node.attribute("EnergyProduction") {
        let number = value
            .parse::<i32>()
            .map_err(|_| invalid(data.path, "invalid EnergyProduction"))?;
        data.word(0x88, number as u32);
    }
    if let Some(value) = node.attribute("CommandSet") {
        data.word(0x90, weak_id(value, "LogicCommandSet", data.path)?);
        reference_types.push(fast_hash(b"LogicCommandSet"));
    }
    for (name, offset) in [("SelectPortrait", 0x94), ("ButtonImage", 0x9c)] {
        if let Some(value) = node.attribute(name) {
            data.word(offset, weak_id(value, "PackedTextureImage", data.path)?);
            reference_types.push(fast_hash(b"PackedTextureImage"));
        }
    }
    if let Some(value) = node.attribute("CampnessValue") {
        let number = value
            .parse::<u32>()
            .map_err(|_| invalid(data.path, "invalid CampnessValue"))?;
        data.word(0xb0, number);
    }
    for (name, offset) in [
        ("CampnessValueRadius", 0xb4),
        ("HealthBoxHeightOffset", 0xbc),
    ] {
        if let Some(value) = node.attribute(name) {
            data.word(offset, finite_float(value, name, data.path)?.to_bits());
        }
    }
    for (name, offset, value, number) in [
        ("ProductionQueueType", 0x12c, "MAIN_STRUCTURE", 0),
        ("BuildPlacementTypeFlag", 0x130, "MAIN_STRUCTURE", 1),
        ("UnitCategory", 0x158, "STRUCTURE", 0),
    ] {
        if let Some(actual) = node.attribute(name) {
            if actual != value {
                return Err(invalid(data.path, format!("invalid {name} {actual}")));
            }
            data.word(offset, number);
        }
    }
    if let Some(value) = node.attribute("WeaponCategory") {
        let category = if value == "NONE" {
            u32::MAX
        } else {
            enum_value(
                value,
                &["GUN", "BLAST", "CANNON", "MISSILE", "LIGHTNING"],
                "WeaponCategory",
                data.path,
            )?
        };
        data.word(0x15c, category);
    }
    Ok(())
}

fn weak_id(value: &str, kind: &str, path: &std::path::Path) -> io::Result<u32> {
    if value.contains(':') {
        return Err(invalid(path, format!("invalid {kind} weak reference")));
    }
    Ok(asset_reference(value, kind, path)?.instance_id)
}

fn finite_float(value: &str, field: &str, path: &std::path::Path) -> io::Result<f32> {
    let number = value
        .parse::<f32>()
        .map_err(|_| invalid(path, format!("invalid {field}")))?;
    if !number.is_finite() {
        return Err(invalid(path, format!("invalid {field}")));
    }
    Ok(number)
}

fn enum_value(
    value: &str,
    values: &[&str],
    field: &str,
    path: &std::path::Path,
) -> io::Result<u32> {
    values
        .iter()
        .position(|option| *option == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("invalid {field} {value}")))
}

pub(super) fn write_kind_of(
    value: &str,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for flag in value.split_ascii_whitespace() {
        let index = kind_of()
            .iter()
            .take(36 * 8)
            .position(|name| name == flag)
            .ok_or_else(|| invalid(data.path, format!("invalid KindOf {flag}")))?;
        data.payload[start + index / 8] |= 1 << (index % 8);
    }
    Ok(())
}

fn kind_of() -> &'static [String] {
    static VALUES: OnceLock<Vec<String>> = OnceLock::new();
    VALUES.get_or_init(|| {
        let doc = Document::parse(include_str!(
            "../../../../schemas/ra3/xsd/Includes/KindOf.xsd"
        ))
        .unwrap();
        doc.descendants()
            .filter(|node| node.tag_name().name() == "enumeration")
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    })
}

fn schema_defaults() -> &'static HashMap<String, String> {
    static VALUES: OnceLock<HashMap<String, String>> = OnceLock::new();
    VALUES.get_or_init(|| {
        let doc = Document::parse(include_str!(
            "../../../../schemas/ra3/xsd/AssetTypeGameObject.xsd"
        ))
        .unwrap();
        doc.descendants()
            .find(|node| {
                node.tag_name().name() == "complexType"
                    && node.attribute("name") == Some("GameObject")
            })
            .unwrap()
            .descendants()
            .filter(|node| node.tag_name().name() == "attribute")
            .filter_map(|node| {
                Some((
                    node.attribute("name")?.to_owned(),
                    node.attribute("default")?.to_owned(),
                ))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;
    use std::path::Path;

    #[test]
    fn allied_root_fields_match_ea_oracle() {
        const BARE: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-bare.bin");
        const ORACLE: &[u8] =
            include_bytes!("../../../../tests/fixtures/game-object-fields-all.bin");
        let document = Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-fields-all.xml"
        ))
        .unwrap();
        let node = document
            .root_element()
            .children()
            .find(|node| node.is_element())
            .unwrap();
        let mut root = BARE[4 + 0x7b4..4 + 0xa00].to_vec();
        root[0x1dc..0x1e0].fill(0);
        let mut data = BinaryData {
            payload: root,
            relocations: Vec::new(),
            path: Path::new("fields.xml"),
        };
        let mut types = Vec::new();
        let mut references = Vec::new();
        encode(node, &mut data, &mut types, &mut references).unwrap();
        for pointer in data.relocations.clone() {
            let at = pointer as usize;
            let value = u32::from_le_bytes(data.payload[at..at + 4].try_into().unwrap());
            data.word(at, value + 0x7b4);
        }
        let mut expected = ORACLE[4 + 0x7b4..4 + 0xa00].to_vec();
        let body_start = u32::from_le_bytes(expected[0x1dc..0x1e0].try_into().unwrap()) as usize;
        expected[0x1dc..0x1e0].fill(0);
        expected.extend_from_slice(&ORACLE[4 + 0xa00..4 + body_start]);
        assert_eq!(data.payload, expected);
        assert_eq!(data.relocations, vec![0x30, 0x40, 0x78]);
        assert!(references.is_empty());
        assert_eq!(
            types,
            vec![0xf231_d60d, 0xec06_6d65, 0x5662_6220, 0x5662_6220]
        );
    }

    #[test]
    fn rejects_unsupported_and_invalid_root_fields() {
        const BARE: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-bare.bin");
        for attribute in [
            "Health=100",
            "KindOf=UNKNOWN_KIND",
            "RadarPriority=UNIT",
            "PlacementViewAngle=NaNd",
            "Side=PlayerTemplate:Allies",
            "BuildTime=NaN",
            "EnergyProduction=1.5",
            "CampnessValue=-1",
            "HealthBoxHeightOffset=inf",
            "ProductionQueueType=OTHER",
            "BuildPlacementTypeFlag=OTHER",
            "UnitCategory=OTHER",
        ] {
            let (name, value) = attribute.split_once('=').unwrap();
            let xml = format!(
                "<GameObject xmlns=\"uri:ea.com:eala:asset\" id=\"Bare\" {name}=\"{value}\" />"
            );
            let document = Document::parse(&xml).unwrap();
            let root = BARE[4 + 0x7b4..4 + 0xa00].to_vec();
            let mut data = BinaryData {
                payload: root,
                relocations: Vec::new(),
                path: Path::new("fields.xml"),
            };
            let result = encode(
                document.root_element(),
                &mut data,
                &mut Vec::new(),
                &mut Vec::new(),
            );
            assert!(result.is_err(), "accepted {attribute}");
        }
    }

    #[test]
    fn accepts_only_schema_defaults_for_unencoded_fields() {
        const BARE: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-bare.bin");
        for (attributes, accepted) in [
            (
                "TransportSlotCount=\"0\" IsTrainable=\"true\" FenceWidth=\"0\"",
                true,
            ),
            ("TransportSlotCount=\"1\"", false),
            ("IsTrainable=\"false\"", false),
            ("FenceWidth=\"1\"", false),
        ] {
            let xml =
                format!("<GameObject xmlns=\"uri:ea.com:eala:asset\" id=\"Bare\" {attributes} />");
            let document = Document::parse(&xml).unwrap();
            let mut data = BinaryData {
                payload: BARE[4 + 0x7b4..4 + 0xa00].to_vec(),
                relocations: Vec::new(),
                path: Path::new("fields.xml"),
            };
            assert_eq!(
                encode(
                    document.root_element(),
                    &mut data,
                    &mut Vec::new(),
                    &mut Vec::new()
                )
                .is_ok(),
                accepted,
                "{attributes}"
            );
        }
    }
}
