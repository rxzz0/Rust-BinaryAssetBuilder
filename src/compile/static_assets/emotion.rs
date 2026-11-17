use super::binary::BinaryData;
use super::{Marshaled, sage_time, schema_enums, validate_attributes, write_import};
use crate::compile::{has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const TIMES: [(&str, usize); 7] = [
    ("DurationActive", 0xc),
    ("DurationInactive", 0x10),
    ("DurationInactiveObject", 0x14),
    ("DurationInactiveType", 0x18),
    ("AttributeModifierStartDelay", 0x3c),
    ("AttributeModifierDuration", 0x40),
    ("AIStateLockDuration", 0x44),
];
const REFERENCES: [(&str, &str, usize); 4] = [
    ("StartFX", "FXList", 0x2c),
    ("UpdateFX", "FXList", 0x30),
    ("EndFX", "FXList", 0x34),
    ("AttributeModifier", "AttributeModifier", 0x38),
];
const CONDITIONS: [(&str, usize); 4] = [
    ("ModelConditionsSetOnStart", 0x4c),
    ("ModelConditionsSetOnEnd", 0x88),
    ("ModelConditionsClearOnStart", 0xc4),
    ("ModelConditionsClearOnEnd", 0x100),
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    static EMOTIONS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    static OPTIONS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    let [emotion_types, ai_states] = EMOTIONS.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/Includes/Emotion.xsd"),
            ["EmotionType", "EmotionAIType"],
        )
    });
    let [options, comparisons] = OPTIONS.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/AssetTypeEmotionTemplate.xsd"),
            [
                "EmotionTemplateOptions",
                "EmotionTemplateThreatComparisonType",
            ],
        )
    });
    let allowed: Vec<_> = [
        "id",
        "Type",
        "Options",
        "EnemyThreatComparison",
        "EnemyThreat",
        "FriendlyThreatComparison",
        "FriendlyThreat",
        "AIState",
        "AILuaEventsList",
    ]
    .into_iter()
    .chain(TIMES.map(|(name, _)| name))
    .chain(REFERENCES.map(|(name, _, _)| name))
    .chain(CONDITIONS.map(|(name, _)| name))
    .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EmotionTemplate content"));
    }
    let mut result = Marshaled::default();
    let mut data = BinaryData {
        payload: vec![0; 0x144],
        relocations: Vec::new(),
        path,
    };
    let ordinal = |value: &str, field: &str, values: &[String]| {
        values
            .iter()
            .position(|name| name == value.trim())
            .map(|i| i as u32)
            .ok_or_else(|| invalid(path, format!("invalid {field} {value:?}")))
    };
    data.word(
        4,
        ordinal(required(node, "Type", path)?, "Type", emotion_types)?,
    );
    let mut mask = 0;
    for option in node
        .attribute("Options")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        mask |= 1 << ordinal(option, "Options", options)?;
    }
    data.word(8, mask);
    for (name, offset) in TIMES {
        let time = sage_time(node.attribute(name).unwrap_or("0s"), name, path)?;
        data.word(offset, time.to_bits());
    }
    for (name, offset) in [
        ("EnemyThreatComparison", 0x1c),
        ("FriendlyThreatComparison", 0x24),
        ("AIState", 0x48),
    ] {
        if let Some(value) = node.attribute(name) {
            let values = if name == "AIState" {
                ai_states
            } else {
                comparisons
            };
            data.word(offset, ordinal(value, name, values)?);
        }
    }
    for (name, offset) in [("EnemyThreat", 0x20), ("FriendlyThreat", 0x28)] {
        if let Some(value) = node.attribute(name) {
            let number = value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?;
            data.word(offset, number as u32);
        }
    }
    for (name, kind, offset) in REFERENCES {
        if let Some(value) = node
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
        {
            write_import(value, kind, offset, &mut data, &mut result)?;
        }
    }
    for (name, offset) in CONDITIONS {
        if let Some(value) = node.attribute(name) {
            super::game_object::write_model_conditions(value, offset, &mut data)?;
        }
    }
    if let Some(value) = node.attribute("AILuaEventsList") {
        data.string(0x13c, value)?;
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}
