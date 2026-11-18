use super::super::binary::BinaryData;
use super::super::{AssetReference, asset_reference, fast_hash, production_queue_type};
use crate::compile::{XML_NS, invalid};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::io;
use std::sync::OnceLock;

const REAL_FIELDS: &[(&str, usize)] = &[
    ("BuildTime", 0x84),
    ("MinZIncreaseForVoiceMoveToHigherGround", 0xa8),
    ("CampnessValueRadius", 0xb4),
    ("Scale", 0xb8),
    ("HealthBoxHeightOffset", 0xbc),
    ("LiveCameraFXPitch", 0xc0),
    ("InstanceScaleFuzziness", 0xcc),
    ("StructureRubbleHeight", 0xd0),
    ("RamZMult", 0xd8),
    ("ShockwaveResistance", 0xdc),
    ("MaxDistanceForEngaged", 0xfc),
    ("ThreatLevel", 0x104),
    ("PathfindDiameter", 0x10c),
    ("FiringArc", 0x11c),
    ("InvisibilityOpacityMin", 0x16c),
    ("InvisibilityOpacityMax", 0x170),
    ("EvaEventSecondDamageFarFromFirstScanRange", 0x17c),
];

const INTEGER_FIELDS: &[(&str, usize)] = &[
    ("BountyValue", 0x80),
    ("EnergyProduction", 0x88),
    ("EnergyBonus", 0x8c),
    ("VoicePriority", 0xa4),
    ("FormationWidth", 0xc4),
    ("FormationDepth", 0xc8),
    ("RamPower", 0xd4),
    ("CommandPoints", 0xe0),
    ("CommandPointBonus", 0xe4),
    ("SupplyOverride", 0x110),
    ("CamouflageDetectorLevel", 0x120),
    ("SelectionPriority", 0x124),
    ("PathPriority", 0x128),
    ("SubGroupPriority", 0x178),
];

const UNSIGNED_FIELDS: &[(&str, usize)] = &[
    ("CampnessValue", 0xb0),
    ("VoiceAttackChargeTimeout", 0xe8),
    ("EngagedStateTimeout", 0x100),
    ("SlopeLimitIndex", 0x108),
];

const TIME_FIELDS: &[(&str, usize)] = &[
    ("JustBuiltTime", 0x64),
    ("VoiceMoveLandToWaterTimeout", 0xec),
    ("VoiceMoveWaterToLandTimeout", 0xf0),
    ("VoiceSelectUnderFireTimeout", 0xf4),
    ("VoiceSelectUnderFireDamageTime", 0xf8),
    ("HasFiredRecentlyTime", 0x160),
    ("ReinvisibilityDelay", 0x168),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
    references: &mut Vec<AssetReference>,
) -> io::Result<()> {
    const ENCODED: &[&str] = &[
        "id",
        "Browser",
        "KindOf",
        "Description",
        "DescriptionTransformed",
        "TypeDescription",
        "TypeDescriptionTransformed",
        "RadarPriority",
        "PlacementViewAngle",
        "Side",
        "EditorName",
        "EditorSorting",
        "MaxSimultaneousOfType",
        "RefundValue",
        "TransportSlotCount",
        "IsTrainable",
        "EvaEventSecondDamageFarFromFirstTimeoutMS",
        "UnitIntro",
        "UnitTypeIcon",
        "CommandSet",
        "SelectPortrait",
        "SelectPortraitTransformed",
        "ButtonImage",
        "ButtonImageTransformed",
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
                || [REAL_FIELDS, INTEGER_FIELDS, UNSIGNED_FIELDS, TIME_FIELDS]
                    .iter()
                    .flat_map(|fields| fields.iter())
                    .any(|&(name, _)| name == attribute.name())
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
    for (name, offset) in [
        ("Description", 0x2c),
        ("DescriptionTransformed", 0x34),
        ("TypeDescription", 0x3c),
        ("TypeDescriptionTransformed", 0x44),
    ] {
        if let Some(value) = node.attribute(name) {
            data.string(offset, value)?;
        }
    }
    if let Some(value) = node.attribute("RadarPriority") {
        data.word(
            0x4c,
            enum_value(
                value,
                &["NOT_ON_RADAR", "INVALID", "STRUCTURE", "UNIT", "COMMANDO"],
                "RadarPriority",
                data.path,
            )?
            .wrapping_sub(2),
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
    for &(name, offset) in TIME_FIELDS {
        if let Some(value) = node.attribute(name) {
            let time = super::super::sage_time(value, name, data.path)?;
            data.word(offset, time.to_bits());
        }
    }
    if let Some(value) = node.attribute("Side") {
        data.word(0x70, weak_id(value, "PlayerTemplate", data.path)?);
        reference_types.push(fast_hash(b"PlayerTemplate"));
    }
    if let Some(value) = node.attribute("EditorName") {
        data.string(0x74, value)?;
    }
    if let Some(value) = node.attribute("EditorSorting") {
        static VALUES: OnceLock<Vec<String>> = OnceLock::new();
        let values = VALUES.get_or_init(|| {
            Document::parse(include_str!(
                "../../../../schemas/ra3/xsd/GameObject/ThingSort.xsd"
            ))
            .unwrap()
            .descendants()
            .find(|node| node.attribute("name") == Some("EditorSortingType"))
            .unwrap()
            .descendants()
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
        });
        let number = values
            .iter()
            .position(|candidate| candidate == value)
            .ok_or_else(|| invalid(data.path, format!("invalid EditorSorting {value}")))?
            as u32;
        data.word(0x7c, number);
    }
    for (name, offset) in [("RefundValue", 0x23c), ("MaxSimultaneousOfType", 0x23e)] {
        if let Some(value) = node.attribute(name) {
            let number = value
                .trim()
                .parse::<u16>()
                .map_err(|_| invalid(data.path, format!("invalid {name}")))?;
            data.payload[offset..offset + 2].copy_from_slice(&number.to_le_bytes());
        }
    }
    if let Some(value) = node.attribute("TransportSlotCount") {
        data.payload[0x240] = value
            .trim()
            .parse::<u8>()
            .map_err(|_| invalid(data.path, "invalid TransportSlotCount"))?;
    }
    data.payload[0x241] = super::draws::boolean(node, "IsTrainable", true, data.path)?;
    for &(name, offset) in INTEGER_FIELDS {
        if let Some(value) = node.attribute(name) {
            let number = value
                .parse::<i32>()
                .map_err(|_| invalid(data.path, format!("invalid {name}")))?;
            data.word(offset, number as u32);
        }
    }
    if let Some(value) = node
        .attribute("CommandSet")
        .filter(|value| !value.trim().is_empty())
    {
        data.word(0x90, weak_id(value, "LogicCommandSet", data.path)?);
        reference_types.push(fast_hash(b"LogicCommandSet"));
    }
    for (name, offset) in [
        ("SelectPortrait", 0x94),
        ("SelectPortraitTransformed", 0x98),
        ("ButtonImage", 0x9c),
        ("ButtonImageTransformed", 0xa0),
    ] {
        if let Some(value) = node
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
        {
            data.word(offset, weak_id(value, "PackedTextureImage", data.path)?);
            reference_types.push(fast_hash(b"PackedTextureImage"));
        }
    }
    for &(name, offset) in UNSIGNED_FIELDS {
        if let Some(value) = node.attribute(name) {
            let number = value
                .parse::<u32>()
                .map_err(|_| invalid(data.path, format!("invalid {name}")))?;
            data.word(offset, number);
        }
    }
    for &(name, offset) in REAL_FIELDS {
        if let Some(value) = node.attribute(name) {
            data.word(offset, finite_float(value, name, data.path)?.to_bits());
        }
    }
    if let Some(value) = node.attribute("EvaEventSecondDamageFarFromFirstTimeoutMS") {
        let milliseconds = value.trim().parse::<u32>().map_err(|_| {
            invalid(
                data.path,
                "invalid EvaEventSecondDamageFarFromFirstTimeoutMS",
            )
        })?;
        data.word(0x180, milliseconds.div_ceil(200));
    }
    for (kind, offset) in [("UnitTypeIcon", 0x164), ("UnitIntro", 0x184)] {
        if let Some(value) = node.attribute(kind) {
            let ordinal = super::passive_area::reference(
                value,
                kind,
                data.path,
                references,
                reference_types,
            )?;
            data.word(offset, ordinal);
        }
    }
    if let Some(value) = node.attribute("ProductionQueueType") {
        data.word(
            0x12c,
            production_queue_type(value, "ProductionQueueType", data.path)?,
        );
    }
    if let Some(value) = node.attribute("BuildPlacementTypeFlag") {
        let placement = enum_value(
            value,
            &[
                "INVALID",
                "MAIN_STRUCTURE",
                "OTHER_STRUCTURE",
                "TIBERIUM_FIELD",
                "BLOCKED",
            ],
            "BuildPlacementTypeFlag",
            data.path,
        )?;
        data.word(0x130, placement);
    }
    if let Some(value) = node.attribute("UnitCategory") {
        let category = enum_value(
            value,
            &["NONE", "STRUCTURE", "INFANTRY", "VEHICLE", "AIRCRAFT"],
            "UnitCategory",
            data.path,
        )?
        .wrapping_sub(1);
        data.word(0x158, category);
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
            "RadarPriority=UNKNOWN",
            "PlacementViewAngle=NaNd",
            "Side=PlayerTemplate:Allies",
            "BuildTime=NaN",
            "EnergyProduction=1.5",
            "CampnessValue=-1",
            "HealthBoxHeightOffset=inf",
            "ThreatLevel=NaN",
            "SlopeLimitIndex=-1",
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
            ("TransportSlotCount=\"1\"", true),
            ("TransportSlotCount=\"256\"", false),
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
