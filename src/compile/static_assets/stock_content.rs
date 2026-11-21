use super::binary::BinaryData;
use super::{Marshaled, significant_children, text_value, validate_attributes, write_import};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn engaging_content(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let names = ["TitleId", "DescId", "ImageId"];
    validate_attributes(node, &["id", "TitleId", "DescId", "ImageId"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EngagingContent content"));
    }
    let mut data = BinaryData {
        payload: vec![0; 28],
        relocations: Vec::new(),
        path,
    };
    for (index, name) in names.into_iter().enumerate() {
        data.string(4 + index * 8, required(node, name, path)?)?;
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn engaging_group(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let items: Vec<_> = significant_children(node).collect();
    if items.is_empty() {
        return Err(invalid(
            path,
            "EngagingContentGroup requires at least one EngagingContent",
        ));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let start = data.list(4, items.len(), 4)?;
    for (index, item) in items.into_iter().enumerate() {
        if !item.has_tag_name((SAGE_NS, "EngagingContent")) || item.attributes().len() != 0 {
            return Err(invalid(path, "unexpected EngagingContentGroup child"));
        }
        write_import(
            text_value(item, path)?,
            "EngagingContent",
            start + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    Ok(data.finish(result))
}

pub(super) fn engaging_sequence(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &["id", "MapName", "RequiredContent", "RandomContent"],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EngagingContentSequence content"));
    }
    let mut data = BinaryData {
        payload: vec![0; 20],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.string(4, required(node, "MapName", path)?)?;
    for (name, offset) in [("RequiredContent", 12), ("RandomContent", 16)] {
        if let Some(value) = node.attribute(name) {
            write_import(
                value,
                "EngagingContentGroup",
                offset,
                &mut data,
                &mut result,
            )?;
        }
    }
    Ok(data.finish(result))
}

pub(super) fn tutorial_map_data(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let lessons: Vec<_> = significant_children(node).collect();
    if lessons.is_empty() {
        return Err(invalid(
            path,
            "TutorialMapData requires at least one Lesson",
        ));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let start = data.list(4, lessons.len(), 24)?;
    let fields = ["Map", "Summary", "ImageName"];
    for (index, lesson) in lessons.into_iter().enumerate() {
        if !lesson.has_tag_name((SAGE_NS, "Lesson")) || has_content(lesson) {
            return Err(invalid(path, "unexpected TutorialMapData child"));
        }
        validate_attributes(lesson, &fields, path)?;
        for (field, name) in fields.into_iter().enumerate() {
            if let Some(value) = lesson.attribute(name) {
                data.string(start + index * 24 + field * 8, value)?;
            }
        }
    }
    Ok(data.finish(Marshaled::default()))
}

#[derive(Clone, Copy)]
enum UiCodec {
    Text,
    Boolean,
    Unsigned,
    Signed,
    Real,
    Hash,
    KindOf,
    Percentage,
    Faction,
    Enum(&'static [&'static str]),
    Weak(&'static str),
    WeakPointer(&'static str),
    Strong(&'static str),
    StrongPointer(&'static str),
    Inline(&'static str),
    Pointer(&'static str),
    Repeated(usize, &'static UiCodec),
}

struct UiField {
    name: &'static str,
    offset: usize,
    attribute: bool,
    default: Option<&'static str>,
    codec: UiCodec,
}

struct UiLayout {
    size: usize,
    fields: &'static [UiField],
}

macro_rules! ui_field {
    ($name:literal, $offset:literal, $attribute:literal, $default:expr, $codec:expr) => {
        UiField {
            name: $name,
            offset: $offset,
            attribute: $attribute,
            default: $default,
            codec: $codec,
        }
    };
}

pub(super) fn ui_config_list(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let configs: Vec<_> = significant_children(node).collect();
    let start = data.list(4, configs.len(), 16)?;
    for (index, config) in configs.into_iter().enumerate() {
        if !config.has_tag_name((SAGE_NS, "UIDynamicConfig")) {
            return Err(invalid(path, "expected UIDynamicConfig in UIConfigList"));
        }
        validate_attributes(config, &["Name", "Type"], path)?;
        let offset = start + 16 * index;
        if let Some(value) = config.attribute("Name") {
            write_ui_value(UiCodec::Hash, value, offset, &mut data, &mut result)?;
        }
        if let Some(value) = config.attribute("Type") {
            write_ui_value(
                UiCodec::Enum(&UICONFIGTYPE),
                value,
                offset + 4,
                &mut data,
                &mut result,
            )?;
        }

        let mut children = significant_children(config);
        let list = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, "ComponentList")))
            .ok_or_else(|| invalid(path, "expected ComponentList in UIDynamicConfig"))?;
        if children.next().is_some() {
            return Err(invalid(path, "unexpected child in UIDynamicConfig"));
        }
        validate_attributes(list, &[], path)?;
        let components: Vec<_> = significant_children(list).collect();
        let first = data.list(offset + 8, components.len(), 4)?;
        for (index, component) in components.into_iter().enumerate() {
            let name = component.tag_name().name();
            if component.tag_name().namespace() != Some(SAGE_NS) || !name.starts_with("UIComponent")
            {
                return Err(invalid(path, format!("unexpected UI component {name}")));
            }
            let layout = require_ui_layout(name, path)?;
            let start = data.allocate(first + 4 * index, layout.size)?;
            data.word(start, crate::hash::fast_hash(name.as_bytes()));
            let priority = required(component, "Priority", path)?;
            write_ui_value(UiCodec::Signed, priority, start + 4, &mut data, &mut result)?;
            write_ui_layout(component, name, layout, start, &mut data, &mut result)?;
        }
    }
    data.relocations.sort_unstable();
    Ok(data.finish(result))
}

fn require_ui_layout(name: &str, path: &Path) -> io::Result<UiLayout> {
    ui_layout(name).ok_or_else(|| invalid(path, format!("unsupported UIConfigList type {name}")))
}

fn write_ui_layout(
    node: Node<'_, '_>,
    name: &str,
    layout: UiLayout,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    match name {
        "ObjectFilter" => {
            return super::game_object::encode_inline_object_filter(
                node,
                offset,
                data,
                &mut result.reference_types,
            )
            .map(|_| ());
        }
        "Color4" => {
            data.word(offset, super::color4(node, data.path)?);
            return Ok(());
        }
        "Vector2" | "RGBColor" => {
            let names: &[_] = if name == "Vector2" {
                &["x", "y"]
            } else {
                &["R", "G", "B"]
            };
            validate_attributes(node, names, data.path)?;
            for (index, name) in names.iter().enumerate() {
                if let Some(value) = node.attribute(*name) {
                    write_ui_value(UiCodec::Real, value, offset + index * 4, data, result)?;
                }
            }
            return Ok(());
        }
        _ => {}
    }

    for attribute in node.attributes() {
        if attribute.namespace() == Some(crate::compile::XML_NS) && attribute.name() == "base" {
            continue;
        }
        if attribute.namespace().is_none()
            && attribute.name() == "Priority"
            && name.starts_with("UIComponent")
        {
            continue;
        }
        if !layout.fields.iter().any(|field| {
            attribute.namespace().is_none() && field.attribute && field.name == attribute.name()
        }) {
            return Err(invalid(
                data.path,
                format!("unexpected {name} attribute {}", attribute.name()),
            ));
        }
    }
    let children: Vec<_> = significant_children(node).collect();
    for child in &children {
        if child.tag_name().namespace() != Some(SAGE_NS)
            || !layout
                .fields
                .iter()
                .any(|field| !field.attribute && child.has_tag_name(field.name))
        {
            return Err(invalid(
                data.path,
                format!("unexpected {name} child {}", child.tag_name().name()),
            ));
        }
    }
    // Import ordinals follow XML attribute order. These fields store an ordinal
    // inline and do not affect allocation order.
    for attribute in node.attributes() {
        if attribute.namespace() == Some(crate::compile::XML_NS) && attribute.name() == "base" {
            continue;
        }
        if let Some(field) = layout.fields.iter().find(|field| {
            field.attribute
                && field.name == attribute.name()
                && matches!(field.codec, UiCodec::Strong(_))
        }) {
            write_ui_value(
                field.codec,
                attribute.value(),
                offset + field.offset,
                data,
                result,
            )?;
        }
    }

    for field in layout.fields {
        if field.attribute && matches!(field.codec, UiCodec::Strong(_)) {
            continue;
        }
        let offset = offset + field.offset;
        if field.attribute {
            if let Some(value) = node.attribute(field.name).or(field.default) {
                write_ui_value(field.codec, value, offset, data, result)?;
            }
        } else {
            let children: Vec<_> = children
                .iter()
                .copied()
                .filter(|child| child.has_tag_name(field.name))
                .collect();
            if let UiCodec::Repeated(stride, codec) = field.codec {
                let start = data.list(offset, children.len(), stride)?;
                for (index, child) in children.into_iter().enumerate() {
                    write_ui_child(*codec, child, start + index * stride, data, result)?;
                }
            } else if let [child] = children.as_slice() {
                write_ui_child(field.codec, *child, offset, data, result)?;
            } else if !children.is_empty() {
                return Err(invalid(
                    data.path,
                    format!("duplicate {name} child {}", field.name),
                ));
            } else if let Some(value) = field.default {
                write_ui_value(field.codec, value, offset, data, result)?;
            }
        }
    }
    Ok(())
}

fn write_ui_child(
    codec: UiCodec,
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    match codec {
        UiCodec::Inline(name) | UiCodec::Pointer(name) => {
            let layout = require_ui_layout(name, data.path)?;
            let start = if matches!(codec, UiCodec::Pointer(_)) {
                data.allocate(offset, layout.size)?
            } else {
                offset
            };
            write_ui_layout(node, name, layout, start, data, result)
        }
        _ => {
            validate_attributes(node, &[], data.path)?;
            if node.children().any(|child| child.is_element()) {
                return Err(invalid(
                    data.path,
                    format!("expected text content in {}", node.tag_name().name()),
                ));
            }
            write_ui_value(codec, node.text().unwrap_or(""), offset, data, result)
        }
    }
}

fn write_ui_value(
    codec: UiCodec,
    value: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    use super::asset_reference;

    let parse_error = || invalid(data.path, format!("invalid UIConfigList value {value:?}"));
    match codec {
        UiCodec::Text => {
            if value.contains(r"\=") {
                data.string(offset, &value.replace(r"\=", "="))?;
            } else {
                data.string(offset, value)?;
            }
        }
        UiCodec::Boolean => {
            data.payload[offset] = match value.trim() {
                "true" | "yes" | "1" => 1,
                "false" | "no" | "0" => 0,
                _ => return Err(parse_error()),
            };
        }
        UiCodec::Unsigned => data.word(
            offset,
            super::sage_uint(value, "UI unsigned integer", data.path)?,
        ),
        UiCodec::Signed => data.word(offset, super::sage_int(value, "UI integer", data.path)?),
        UiCodec::Real => data.word(
            offset,
            super::finite_number(value, "UI real number", data.path)?.to_bits(),
        ),
        UiCodec::Hash => {
            let bytes = crate::compile::windows_1252(value).ok_or_else(parse_error)?;
            data.word(offset, crate::hash::asset_string_hash(&bytes));
        }
        UiCodec::Percentage => data.word(
            offset,
            super::percentage(value, "UI opacity", data.path)?.to_bits(),
        ),
        UiCodec::Faction => {
            let index = FACTIONTYPE
                .iter()
                .position(|&variant| variant == value.trim())
                .ok_or_else(parse_error)?;
            data.word(offset, (index as u32).wrapping_sub(1));
        }
        UiCodec::Enum(variants) => {
            let index = variants
                .iter()
                .position(|&variant| variant == value.trim())
                .ok_or_else(parse_error)?;
            data.word(offset, index as u32);
        }
        UiCodec::KindOf => {
            for (index, word) in super::global_ui::kind_of_flags(value, data.path)?
                .into_iter()
                .enumerate()
            {
                data.word(offset + index * 4, word);
            }
        }
        UiCodec::Weak(_)
        | UiCodec::WeakPointer(_)
        | UiCodec::Strong(_)
        | UiCodec::StrongPointer(_)
            if value.trim().is_empty() => {}
        UiCodec::Weak(kind) | UiCodec::WeakPointer(kind) => {
            let reference = asset_reference(value, kind, data.path)?;
            result.reference_types.push(reference.type_id);
            if matches!(codec, UiCodec::WeakPointer(_)) {
                data.optional_word(offset, reference.instance_id)?;
            } else {
                data.word(offset, reference.instance_id);
            }
        }
        UiCodec::Strong(kind) => write_import(value, kind, offset, data, result)?,
        UiCodec::StrongPointer(kind) => {
            let start = data.allocate(offset, 4)?;
            write_import(value, kind, start, data, result)?;
        }
        UiCodec::Inline(_) | UiCodec::Pointer(_) | UiCodec::Repeated(_, _) => {
            return Err(parse_error());
        }
    }
    Ok(())
}

// The layout tables use the RA3 XML compiler's member order, byte offsets and
// collection strides. XML schema order can differ from allocation order.

const JOYPADBUTTONTYPE: [&str; 14] = [
    "LEFT_STICK",
    "LEFT_TRIGGER",
    "LEFT_BUMPER",
    "RIGHT_STICK",
    "RIGHT_TRIGGER",
    "RIGHT_BUMPER",
    "A",
    "B",
    "X",
    "Y",
    "D_PAD",
    "BACK",
    "START",
    "GUIDE",
];
const JOYPADACTIONTYPE: [&str; 15] = [
    "PRESS",
    "HOLD",
    "CLICK",
    "DOUBLE_CLICK",
    "ON_GROUND",
    "ON_TARGET",
    "ON_UNIT_TYPE",
    "MOVE",
    "LEFT",
    "RIGHT",
    "UP",
    "DOWN",
    "LEFT_OR_RIGHT",
    "UP_OR_DOWN",
    "LINK_WITH_NEXT",
];
const SHADOWTYPE: [&str; 7] = [
    "NONE",
    "DECAL",
    "ALPHA_DECAL",
    "ADDITIVE_DECAL",
    "ALPHA_DECAL_DYNAMIC",
    "ADDITIVE_DECAL_DYNAMIC",
    "MERGE_DECAL",
];
const GAMEPLAYTYPE: [&str; 5] = ["VERSUS", "KOTH", "CAH", "CTF", "SIEGE"];
const FACTIONTYPE: [&str; 5] = ["RANDOM", "Allies", "Soviet", "Japan", "Neutral"];
const AIDIFFICULTYVALUE: [&str; 4] = ["EASY", "MEDIUM", "HARD", "BRUTAL"];
const SLOTSTATETYPE: [&str; 4] = ["OPEN", "CLOSED", "PRIVATE", "AI"];
const CAREERSTATTYPE: [&str; 3] = ["Int", "Float", "Time"];
const CAREERSTATORDER: [&str; 35] = [
    "CurrentWinStreak",
    "CurrentLossStreak",
    "LongestWinStreak",
    "LongestLosingStreak",
    "TotalMatchesPlayed",
    "CareerLosses",
    "CareerWins",
    "WinLossRatio",
    "AvgGameLength",
    "TotalTimePlayed",
    "StructuresBuilt",
    "StructuresLost",
    "StructuresDestroyed",
    "StructuresCaptured",
    "StructuresCapturedByEnemy",
    "CaptureRatio",
    "SuperWeaponsBuilt",
    "BridgesDestroyed",
    "StructuresSold",
    "UnitsBuilt",
    "UnitsLost",
    "UnitsKilled",
    "UnitKillLostRatio",
    "AvgIncomePerMinute",
    "SecondsInLowPower",
    "ResourcesGathered",
    "ResourcesSpent",
    "FormationPreviewUsed",
    "EfficiencyRatio",
    "ValueOfUnitsLost",
    "ValueOfUnitsKilled",
    "FriendlyFireIncidents",
    "ActivatedUnitAbilities",
    "ActivatedCommanderPowers",
    "StructuresInfiltrated",
];
const POSTGAMESTATTYPE: [&str; 17] = [
    "SessionLength",
    "StructuresCreated",
    "StructuresLost",
    "StructuresDestroyed",
    "StructuresCaptured",
    "StructuresSold",
    "UnitsCreated",
    "UnitsLost",
    "UnitsKilled",
    "UnitKillDeathRatio",
    "AvgIncomePerMinute",
    "AvgIncomePerSecond",
    "ResourcesGathered",
    "EfficiencyRatio",
    "EnemyStructuresInfiltrated",
    "UnitAbilitiesUsed",
    "CommanderPowersUsed",
];
const UICONFIGTYPE: [&str; 4] = ["INGAME", "SHELL", "BOOTUP", "LOADSCREEN"];
fn ui_layout(name: &str) -> Option<UiLayout> {
    use UiCodec::*;
    let (size, fields): (_, &[_]) = match name {
        "AIDifficultySetting" => (
            12,
            &[
                ui_field!("Label", 0, true, None, Text),
                ui_field!("Value", 8, true, None, Enum(&AIDIFFICULTYVALUE)),
            ],
        ),
        "ArchivableMovie" => (
            32,
            &[
                ui_field!("ProgressLock", 0, true, Some("0"), Unsigned),
                ui_field!("MovieImage", 4, true, None, Strong("OnDemandTextureImage")),
                ui_field!("FileName", 8, true, None, Text),
                ui_field!("OnScreenName", 16, true, None, Text),
                ui_field!("Description", 24, true, None, Text),
            ],
        ),
        "ArchiveMovieList" => (
            12,
            &[
                ui_field!("Faction", 0, true, Some("Soviet"), Faction),
                ui_field!(
                    "Movie",
                    4,
                    false,
                    None,
                    Repeated(32, &Inline("ArchivableMovie"))
                ),
            ],
        ),
        "CareerStatData" => (
            24,
            &[
                ui_field!("StatType", 0, true, None, Enum(&CAREERSTATTYPE)),
                ui_field!("StatLabel", 4, true, None, Text),
                ui_field!("IsRatio", 20, true, Some("false"), Boolean),
                ui_field!("NoSummary", 21, true, Some("false"), Boolean),
                ui_field!("RatioNumerator", 12, true, None, Enum(&CAREERSTATORDER)),
                ui_field!("RatioDenominator", 16, true, None, Enum(&CAREERSTATORDER)),
            ],
        ),
        "Color4" => (4, &[]),
        "DefaultValues" => (
            72,
            &[
                ui_field!("Difficulty", 0, true, Some("1"), Signed),
                ui_field!("Gamma", 4, true, Some("1.30"), Real),
                ui_field!("Brightness", 8, true, Some("0.50"), Real),
                ui_field!("Contrast", 12, true, Some("1.30"), Real),
                ui_field!("VolumeMusic", 16, true, Some("0.45"), Real),
                ui_field!("VolumeFX", 20, true, Some("0.70"), Real),
                ui_field!("VolumeVoice", 24, true, Some("0.70"), Real),
                ui_field!("VolumeAmbient", 28, true, Some("0.50"), Real),
                ui_field!("VolumeMovie", 32, true, Some("0.70"), Real),
                ui_field!("ScrollSpeed", 36, true, Some("1.00"), Real),
                ui_field!("ScrollMagnetism", 40, true, Some("2"), Signed),
                ui_field!("ButtonIcon", 60, true, Some("true"), Boolean),
                ui_field!("ShowCommandList", 61, true, Some("true"), Boolean),
                ui_field!("HealthBars", 62, true, Some("true"), Boolean),
                ui_field!("HighlightPads", 63, true, Some("true"), Boolean),
                ui_field!("ToolTips", 64, true, Some("true"), Boolean),
                ui_field!("Subtitles", 65, true, Some("false"), Boolean),
                ui_field!("InvertRotate", 66, true, Some("true"), Boolean),
                ui_field!("Telemetry", 67, true, Some("true"), Boolean),
                ui_field!("DefaultFaction", 44, true, Some("-1"), Signed),
                ui_field!("VisionCamVisible", 68, true, Some("true"), Boolean),
                ui_field!("VisionCamZoom", 48, true, Some("1"), Signed),
                ui_field!("ScrollSpeedMin", 52, true, Some("0.2"), Real),
                ui_field!("ScrollSpeedMax", 56, true, Some("2.0"), Real),
            ],
        ),
        "FontDesc" => (
            16,
            &[
                ui_field!("Name", 0, true, None, Text),
                ui_field!("Size", 8, true, None, Unsigned),
                ui_field!("Bold", 12, true, Some("false"), Boolean),
            ],
        ),
        "GameFlagsSetting" => (
            8,
            &[
                ui_field!("Value", 0, true, None, Signed),
                ui_field!("Default", 4, true, Some("false"), Boolean),
            ],
        ),
        "GameplayTypeSetting" => (
            16,
            &[
                ui_field!("Label", 0, true, None, Text),
                ui_field!("Value", 8, true, None, Enum(&GAMEPLAYTYPE)),
                ui_field!("AvailableWithAI", 12, true, Some("false"), Boolean),
            ],
        ),
        "GamePointsSetting" => (
            8,
            &[
                ui_field!("Value", 0, true, None, Signed),
                ui_field!("Default", 4, true, Some("false"), Boolean),
            ],
        ),
        "GameResourcesSetting" => (
            8,
            &[
                ui_field!("Value", 0, true, None, Signed),
                ui_field!("Default", 4, true, Some("false"), Boolean),
            ],
        ),
        "GameTimeSetting" => (
            20,
            &[
                ui_field!("Label", 0, true, None, Text),
                ui_field!("Value", 8, true, None, Signed),
                ui_field!("Default", 16, true, Some("false"), Boolean),
                ui_field!("MinGameTimeValue", 12, true, Some("0"), Signed),
            ],
        ),
        "InputButton" => (
            8,
            &[
                ui_field!("ButtonName", 0, true, None, Enum(&JOYPADBUTTONTYPE)),
                ui_field!("ButtonAction", 4, true, None, Enum(&JOYPADACTIONTYPE)),
            ],
        ),
        "MapListType" => (
            8,
            &[ui_field!(
                "Map",
                0,
                false,
                None,
                Repeated(4, &Weak("MetaDataObject"))
            )],
        ),
        "MultiplayerLobbyData" => (
            48,
            &[
                ui_field!("LobbyTitleLabelSpec", 0, false, None, Text),
                ui_field!("UnmoderatedLobbyTitleTemplateSpec", 8, false, None, Text),
                ui_field!("ModeratedLobbyTitleTemplateSpec", 16, false, None, Text),
                ui_field!("PlayerNameLabelSpec", 24, false, None, Text),
                ui_field!("SlotLabelSpec", 32, false, None, Text),
                ui_field!(
                    "SlotState",
                    40,
                    false,
                    None,
                    Repeated(16, &Inline("SlotState"))
                ),
            ],
        ),
        "ObjectFilter" => (112, &[]),
        "PostGameStatData" => (
            12,
            &[
                ui_field!("StatType", 0, true, None, Enum(&POSTGAMESTATTYPE)),
                ui_field!("StatLabel", 4, true, None, Text),
            ],
        ),
        "RadiusDecalComponent" => (
            64,
            &[
                ui_field!("Texture", 0, true, None, Strong("Texture")),
                ui_field!("Texture2", 4, true, None, Strong("Texture")),
                ui_field!("Style", 8, true, None, Enum(&SHADOWTYPE)),
                ui_field!("OpacityMin", 12, true, Some("1.0"), Percentage),
                ui_field!("OpacityMax", 16, true, Some("1.0"), Percentage),
                ui_field!("OpacityPulsesPerSecond", 20, true, Some("0.0"), Real),
                ui_field!("RotationMin", 24, true, Some("0.0"), Real),
                ui_field!("RotationMax", 28, true, Some("0.0"), Real),
                ui_field!("RotationDegreesPerSecond", 32, true, Some("0.0"), Real),
                ui_field!("MinRadius", 36, true, Some("1.0"), Real),
                ui_field!("MaxRadius", 40, true, Some("1.0"), Real),
                ui_field!("RadiusPulsesPerSecond", 44, true, Some("0.0"), Real),
                ui_field!("PositionOffsetMin", 48, true, Some("0.0"), Real),
                ui_field!("PositionOffsetMax", 52, true, Some("0.0"), Real),
                ui_field!("PositionOffsetPulsesPerSecond", 56, true, Some("0.0"), Real),
                ui_field!("Color", 60, false, None, Inline("Color4")),
            ],
        ),
        "RadiusDecalTemplate" => (
            8,
            &[ui_field!(
                "Component",
                0,
                false,
                None,
                Repeated(64, &Inline("RadiusDecalComponent"))
            )],
        ),
        "RankedOptionSetting" => (
            12,
            &[
                ui_field!("Label", 0, true, None, Text),
                ui_field!("Value", 8, true, None, Signed),
            ],
        ),
        "RGBColor" => (12, &[]),
        "SingleCommandLine" => (
            24,
            &[
                ui_field!("Purpose", 0, true, None, Text),
                ui_field!("Description", 8, true, None, Text),
                ui_field!(
                    "Input",
                    16,
                    false,
                    None,
                    Repeated(8, &Inline("InputButton"))
                ),
            ],
        ),
        "SingleCommandList" => (
            16,
            &[
                ui_field!("CategoryName", 0, true, None, Text),
                ui_field!(
                    "CommandLine",
                    8,
                    false,
                    None,
                    Repeated(24, &Inline("SingleCommandLine"))
                ),
            ],
        ),
        "SkirmishLobbyData" => (
            24,
            &[
                ui_field!("LoadMusic", 0, false, None, Text),
                ui_field!("SlotLabelSpec", 8, false, None, Text),
                ui_field!(
                    "SlotState",
                    16,
                    false,
                    None,
                    Repeated(16, &Inline("SlotState"))
                ),
            ],
        ),
        "SlotState" => (
            16,
            &[
                ui_field!("Label", 0, true, None, Text),
                ui_field!("Value", 8, true, None, Enum(&SLOTSTATETYPE)),
                ui_field!("AvailableInRanked", 12, true, Some("true"), Boolean),
                ui_field!("AvailableInModerated", 13, true, Some("true"), Boolean),
            ],
        ),
        "TeamSetting" => (
            12,
            &[
                ui_field!("Label", 0, true, None, Text),
                ui_field!("Value", 8, true, None, Signed),
            ],
        ),
        "UIComponentAptJoypad" => (
            12,
            &[
                ui_field!("RelayLeftStickPosition", 8, true, Some("false"), Boolean),
                ui_field!("RelayRightStickPosition", 9, true, Some("false"), Boolean),
            ],
        ),
        "UIComponentAptMediator" => (
            104,
            &[
                ui_field!("AptGlobalLoadVarName", 8, true, None, Text),
                ui_field!("AptEventHandlerName", 16, true, None, Text),
                ui_field!("AptFSCommandHandlerName", 24, true, None, Text),
                ui_field!("AptTokenDelimiters", 32, true, None, Text),
                ui_field!("NewSkoolIndicator", 40, true, None, Text),
                ui_field!("FunctionSeparator", 48, true, None, Text),
                ui_field!("ValueSeparator", 56, true, None, Text),
                ui_field!("InputParameterSeparator", 64, true, None, Text),
                ui_field!("ParameterNameSeparator", 72, true, None, Text),
                ui_field!("ParameterStartIndicator", 80, true, None, Text),
                ui_field!("SubObjectSeparator", 88, true, None, Text),
                ui_field!("OutputParameterSeparator", 96, true, None, Text),
            ],
        ),
        "UIComponentBootup" => (
            24,
            &[
                ui_field!("RenderImageName", 8, true, None, Text),
                ui_field!("LoadingTextImageName", 16, true, None, Text),
            ],
        ),
        "UIComponentCareerStats" => (
            20,
            &[
                ui_field!(
                    "DefaultRankImage",
                    8,
                    true,
                    None,
                    Strong("PackedTextureImage")
                ),
                ui_field!(
                    "CareerStat",
                    12,
                    false,
                    None,
                    Repeated(24, &Inline("CareerStatData"))
                ),
            ],
        ),
        "UIComponentChat" => (
            24,
            &[
                ui_field!("ChatTextLocalLabelPrefix", 8, true, None, Text),
                ui_field!("ChatTextGlobalLabelPrefix", 16, true, None, Text),
            ],
        ),
        "UIComponentComlinkNotification" => {
            (16, &[ui_field!("MessageTokenSpec", 8, false, None, Text)])
        }
        "UIComponentCommandBar" => (
            184,
            &[
                ui_field!("HomeButtonImageName", 8, true, Some("CbarHome"), Text),
                ui_field!("ButtonSlotImageBaseName", 16, true, Some("CbarSlot"), Text),
                ui_field!(
                    "GeneralButtonImageBaseName",
                    24,
                    true,
                    Some("CbarGeneral"),
                    Text
                ),
                ui_field!("RenderClockName", 32, true, None, Text),
                ui_field!("ClockScale", 40, true, None, Real),
                ui_field!("ClockImage", 44, true, None, Weak("PackedTextureImage")),
                ui_field!("UnitCapFlashSeconds", 48, true, None, Signed),
                ui_field!("UnitCapFinalWarningAmount", 52, true, None, Signed),
                ui_field!("UnitCapInitialWarningAmount", 56, true, None, Signed),
                ui_field!("UnitCapNoMoreCP", 60, true, None, Text),
                ui_field!("UnitCapAlmostNoMoreCP", 68, true, None, Text),
                ui_field!("ScrollPowersSoundName", 76, true, None, Text),
                ui_field!("OpenUISoundName", 84, true, None, Text),
                ui_field!("CloseUISoundName", 92, true, None, Text),
                ui_field!("HighlightButtonSoundName", 100, true, None, Text),
                ui_field!(
                    "HighlightButtonForceFeedback",
                    108,
                    true,
                    None,
                    WeakPointer("ForceFeedbackEvent")
                ),
                ui_field!(
                    "TransitionForceFeedback",
                    112,
                    true,
                    None,
                    WeakPointer("ForceFeedbackEvent")
                ),
                ui_field!("SelectButtonSoundName", 116, true, None, Text),
                ui_field!("ZoomInSoundName", 124, true, None, Text),
                ui_field!("ZoomOutSoundName", 132, true, None, Text),
                ui_field!("CycleSoundName", 140, true, None, Text),
                ui_field!("AptTokenTitle", 148, true, None, Text),
                ui_field!("AptTokenCost", 156, true, None, Text),
                ui_field!("AptTokenTime", 164, true, None, Text),
                ui_field!("AptTokenEnergy", 172, true, None, Text),
                ui_field!("ClockColor", 180, false, None, Inline("Color4")),
            ],
        ),
        "UIComponentCommandList" => (
            56,
            &[
                ui_field!("AptTokenLeftTrigger", 8, true, None, Text),
                ui_field!("AptTokenRightTrigger", 16, true, None, Text),
                ui_field!("AptTokenAButton", 24, true, None, Text),
                ui_field!("AptTokenBButton", 32, true, None, Text),
                ui_field!("AptTokenXButton", 40, true, None, Text),
                ui_field!("AptTokenYButton", 48, true, None, Text),
            ],
        ),
        "UIComponentCoopCommander" => (
            88,
            &[
                ui_field!("AssaultButtonName", 8, true, None, Text),
                ui_field!("AssaultButtonDesc", 16, true, None, Text),
                ui_field!(
                    "AssaultButtonCommandBarButtonTemplate",
                    24,
                    true,
                    None,
                    Weak("JoypadCommandBarButtonTemplate")
                ),
                ui_field!("NormalButtonName", 28, true, None, Text),
                ui_field!("NormalButtonDesc", 36, true, None, Text),
                ui_field!(
                    "NormalButtonCommandBarButtonTemplate",
                    44,
                    true,
                    None,
                    Weak("JoypadCommandBarButtonTemplate")
                ),
                ui_field!("HoldPosButtonName", 48, true, None, Text),
                ui_field!("HoldPosButtonDesc", 56, true, None, Text),
                ui_field!(
                    "HoldPosButtonCommandBarButtonTemplate",
                    64,
                    true,
                    None,
                    Weak("JoypadCommandBarButtonTemplate")
                ),
                ui_field!("HoldObjButtonName", 68, true, None, Text),
                ui_field!("HoldObjButtonDesc", 76, true, None, Text),
                ui_field!(
                    "HoldObjButtonCommandBarButtonTemplate",
                    84,
                    true,
                    None,
                    Weak("JoypadCommandBarButtonTemplate")
                ),
            ],
        ),
        "UIComponentCursor" => (
            16,
            &[
                ui_field!("AptEventCursorChanged", 8, true, None, Hash),
                ui_field!(
                    "HoverSelectForceFeedback",
                    12,
                    true,
                    None,
                    WeakPointer("ForceFeedbackEvent")
                ),
            ],
        ),
        "UIComponentFormation" => (12, &[ui_field!("MaxDragLength", 8, true, None, Real)]),
        "UIComponentGameInfo" => (
            120,
            &[
                ui_field!("GameScoreDisplayToken", 8, true, None, Text),
                ui_field!("ScorePlayerToken", 16, true, None, Text),
                ui_field!("FactionLabelToken", 24, true, None, Text),
                ui_field!("GameScoreXLocWS", 32, true, None, Signed),
                ui_field!("GameScoreXLoc", 36, true, None, Signed),
                ui_field!("GameScoreYLocWS", 40, true, None, Signed),
                ui_field!("GameScoreYLoc", 44, true, None, Signed),
                ui_field!("Player1ScoreXLocWS", 48, true, None, Signed),
                ui_field!("Player1ScoreXLoc", 52, true, None, Signed),
                ui_field!("Player1ScoreYLocWS", 56, true, None, Signed),
                ui_field!("Player1ScoreYLoc", 60, true, None, Signed),
                ui_field!("Player2ScoreXLocWS", 64, true, None, Signed),
                ui_field!("Player2ScoreXLoc", 68, true, None, Signed),
                ui_field!("Player2ScoreYLocWS", 72, true, None, Signed),
                ui_field!("Player2ScoreYLoc", 76, true, None, Signed),
                ui_field!("Player3ScoreXLocWS", 80, true, None, Signed),
                ui_field!("Player3ScoreXLoc", 84, true, None, Signed),
                ui_field!("Player3ScoreYLocWS", 88, true, None, Signed),
                ui_field!("Player3ScoreYLoc", 92, true, None, Signed),
                ui_field!("Player4ScoreXLocWS", 96, true, None, Signed),
                ui_field!("Player4ScoreXLoc", 100, true, None, Signed),
                ui_field!("Player4ScoreYLocWS", 104, true, None, Signed),
                ui_field!("Player4ScoreYLoc", 108, true, None, Signed),
                ui_field!("FactionValue", 112, false, None, Repeated(8, &Text)),
            ],
        ),
        "UIComponentGameViewJoypad" => (
            116,
            &[
                ui_field!("LeftStickMultiplier", 8, true, Some("100.0"), Real),
                ui_field!("RightStickMultiplier", 12, true, Some("0.25"), Real),
                ui_field!("ObjectAttractivityForce", 16, true, Some("20.0"), Real),
                ui_field!("MinAttractDistance", 20, true, Some("40.0"), Real),
                ui_field!("MinAttractDistanceZoomed", 24, true, Some("40.0"), Real),
                ui_field!("MaxAttractiveForce", 28, true, Some("0.6"), Real),
                ui_field!("KindOfExemptFromAttraction", 32, true, None, KindOf),
                ui_field!("ScrollSpeedMin", 68, true, None, Real),
                ui_field!("ScrollSpeedMax", 72, true, None, Real),
                ui_field!("ScrollSpeedMaxCutoff", 76, true, None, Real),
                ui_field!("ScrollSpeedMinCutoff", 80, true, None, Real),
                ui_field!("InternalZoomInValue", 84, true, None, Real),
                ui_field!("InternalZoomOutValue", 88, true, None, Real),
                ui_field!("RapidScrollMultiplier", 92, true, Some("2.0"), Real),
                ui_field!("MagnetismTuning_LOW_Strength", 96, true, Some("1.1"), Real),
                ui_field!("MagnetismTuning_LOW_Distance", 100, true, Some("0.6"), Real),
                ui_field!(
                    "MagnetismTuning_HIGH_Strength",
                    104,
                    true,
                    Some("1.5"),
                    Real
                ),
                ui_field!(
                    "MagnetismTuning_HIGH_Distance",
                    108,
                    true,
                    Some("1.25"),
                    Real
                ),
                ui_field!(
                    "MagnetismObjectFilter",
                    112,
                    false,
                    None,
                    Pointer("ObjectFilter")
                ),
            ],
        ),
        "UIComponentInGameText" => (
            168,
            &[
                ui_field!("AdjustFactor", 8, true, None, Real),
                ui_field!("TextIndent", 12, true, None, Real),
                ui_field!("TooltipAppearDelayMS", 16, true, Some("3000"), Unsigned),
                ui_field!("TooltipDisappearDelayMS", 20, true, Some("3000"), Unsigned),
                ui_field!("AptTokenTitle", 24, true, None, Text),
                ui_field!("AptTokenPrereq", 32, true, None, Text),
                ui_field!("AptTokenCost", 40, true, None, Text),
                ui_field!("AptTokenTime", 48, true, None, Text),
                ui_field!("AptTokenEnergy", 56, true, None, Text),
                ui_field!("AptTokenShortDesc", 64, true, None, Text),
                ui_field!("AptTokenLongDesc", 72, true, None, Text),
                ui_field!("ProductionTextConstruction", 80, true, None, Text),
                ui_field!("ProductionTextUpgrade", 88, true, None, Text),
                ui_field!("ProductionTextRecruit", 96, true, None, Text),
                ui_field!("ProductionTextScale", 104, true, None, Real),
                ui_field!("SubtitleStringLabelPrefix", 108, true, None, Text),
                ui_field!("SubtitleStringLabelSuffix", 116, true, None, Text),
                ui_field!("SubtitleStringExclusionChar", 124, true, None, Text),
                ui_field!("StatusTextPosition", 132, false, None, Inline("Vector2")),
                ui_field!("ProductionTextColor", 140, false, None, Inline("RGBColor")),
                ui_field!("ProductionTextFont", 152, false, None, Inline("FontDesc")),
            ],
        ),
        "UIComponentKeyboard" => (
            16,
            &[ui_field!("EditableTextLabelPrefix", 8, true, None, Text)],
        ),
        "UIComponentLanLobby" => (
            40,
            &[
                ui_field!("PlayerlistMinUpdateFreq", 8, false, None, Unsigned),
                ui_field!("GamelistMinUpdateFreq", 12, false, None, Unsigned),
                ui_field!("PlayerNameAptTokenSpec", 16, false, None, Text),
                ui_field!("GameNameAptTokenSpec", 24, false, None, Text),
                ui_field!("LocalPlayerNameAptTokenSpec", 32, false, None, Text),
            ],
        ),
        "UIComponentLobby" => (
            264,
            &[
                ui_field!("RandomStringLabel", 8, false, None, Text),
                ui_field!("FactionLabelSpec", 16, false, None, Text),
                ui_field!("AIPersonalityLabelSpec", 24, false, None, Text),
                ui_field!("AIDifficultyLabelSpec", 32, false, None, Text),
                ui_field!("TeamLabelSpec", 40, false, None, Text),
                ui_field!("MapListSpec", 48, false, None, Text),
                ui_field!("GameplayTypeSpec", 56, false, None, Text),
                ui_field!("GameTimeLimitSpec", 64, false, None, Text),
                ui_field!("GameResourcesSpec", 72, false, None, Text),
                ui_field!("HillTimeSpec", 80, false, None, Text),
                ui_field!("CapturePointsSpec", 88, false, None, Text),
                ui_field!("BarrierTimeSpec", 96, false, None, Text),
                ui_field!("CaptureFlagsSpec", 104, false, None, Text),
                ui_field!("Faction", 112, false, None, Repeated(4, &Faction)),
                ui_field!(
                    "AIDifficulty",
                    120,
                    false,
                    None,
                    Repeated(12, &Inline("AIDifficultySetting"))
                ),
                ui_field!(
                    "Team",
                    128,
                    false,
                    None,
                    Repeated(12, &Inline("TeamSetting"))
                ),
                ui_field!(
                    "GameType",
                    136,
                    false,
                    None,
                    Repeated(16, &Inline("GameplayTypeSetting"))
                ),
                ui_field!(
                    "GameTime",
                    144,
                    false,
                    None,
                    Repeated(20, &Inline("GameTimeSetting"))
                ),
                ui_field!(
                    "GameResources",
                    152,
                    false,
                    None,
                    Repeated(8, &Inline("GameResourcesSetting"))
                ),
                ui_field!(
                    "HillTime",
                    160,
                    false,
                    None,
                    Repeated(20, &Inline("GameTimeSetting"))
                ),
                ui_field!(
                    "BarrierTime",
                    168,
                    false,
                    None,
                    Repeated(20, &Inline("GameTimeSetting"))
                ),
                ui_field!(
                    "CapturePoints",
                    176,
                    false,
                    None,
                    Repeated(8, &Inline("GamePointsSetting"))
                ),
                ui_field!(
                    "CaptureFlags",
                    184,
                    false,
                    None,
                    Repeated(8, &Inline("GameFlagsSetting"))
                ),
                ui_field!(
                    "MultiplayerLobbySettings",
                    192,
                    false,
                    None,
                    Inline("MultiplayerLobbyData")
                ),
                ui_field!(
                    "SkirmishLobbySettings",
                    240,
                    false,
                    None,
                    Inline("SkirmishLobbyData")
                ),
            ],
        ),
        "UIComponentMapList" => (16, &[ui_field!("MapListLabelPrefix", 8, false, None, Text)]),
        "UIComponentMovie" => (
            20,
            &[
                ui_field!("MovieComponentName", 8, true, None, Text),
                ui_field!("MaxNumberOfStream", 16, true, Some("3"), Signed),
            ],
        ),
        "UIComponentMovieArchive" => (
            24,
            &[
                ui_field!("MissionSpec", 8, true, None, Text),
                ui_field!(
                    "FactionMovieArchive",
                    16,
                    false,
                    None,
                    Repeated(12, &Inline("ArchiveMovieList"))
                ),
            ],
        ),
        "UIComponentObjectives" => (
            24,
            &[
                ui_field!("AptStringNewBonusObjective", 8, true, Some(""), Text),
                ui_field!("AptStringObjectiveCompleted", 16, true, Some(""), Text),
            ],
        ),
        "UIComponentObjectSelector" => (
            116,
            &[
                ui_field!("ShowAllHealthBars", 112, true, Some("true"), Boolean),
                ui_field!("PortraitName", 8, true, Some("true"), Text),
                ui_field!("UnitAbility", 16, true, Some("true"), Text),
                ui_field!("StatusTextNothingSelected", 24, true, None, Text),
                ui_field!("StatusTextSelectedAcrossMap", 32, true, None, Text),
                ui_field!("StatusTextSelectedAcrossScreen", 40, true, None, Text),
                ui_field!(
                    "AllArmyImageGDI",
                    48,
                    true,
                    None,
                    Weak("PackedTextureImage")
                ),
                ui_field!(
                    "AllArmyImageNOD",
                    52,
                    true,
                    None,
                    Weak("PackedTextureImage")
                ),
                ui_field!(
                    "AllArmyImageAlien",
                    56,
                    true,
                    None,
                    Weak("PackedTextureImage")
                ),
                ui_field!("ClockImage", 60, true, None, Weak("PackedTextureImage")),
                ui_field!(
                    "AssaultTargetForceFeedback",
                    64,
                    true,
                    None,
                    WeakPointer("ForceFeedbackEvent")
                ),
                ui_field!(
                    "AlienHighlightDecal",
                    68,
                    false,
                    None,
                    Inline("RadiusDecalTemplate")
                ),
                ui_field!(
                    "GDIHighlightDecal",
                    76,
                    false,
                    None,
                    Inline("RadiusDecalTemplate")
                ),
                ui_field!(
                    "NODHighlightDecal",
                    84,
                    false,
                    None,
                    Inline("RadiusDecalTemplate")
                ),
                ui_field!(
                    "ShortcutObjects",
                    92,
                    false,
                    None,
                    Repeated(112, &Inline("ObjectFilter"))
                ),
                ui_field!(
                    "SupportObjects",
                    100,
                    false,
                    None,
                    Repeated(112, &Inline("ObjectFilter"))
                ),
                ui_field!("ClockColor", 108, false, None, Inline("Color4")),
            ],
        ),
        "UIComponentOnlineBuddyList" => (
            24,
            &[
                ui_field!("BuddyNameAptTokenSpec", 8, false, None, Text),
                ui_field!("BuddyLocAptTokenSpec", 16, false, None, Text),
            ],
        ),
        "UIComponentOnlineGameList" => (
            28,
            &[
                ui_field!("GameNameAptTokenSpec", 8, false, None, Text),
                ui_field!("GameMapNameAptTokenSpec", 16, false, None, Text),
                ui_field!("GameListRefreshInterval", 24, false, None, Signed),
            ],
        ),
        "UIComponentOnlineInvite" => (
            20,
            &[
                ui_field!("MaxNumberLoggedInvites", 8, false, None, Unsigned),
                ui_field!("InviteNameAptTokenSpec", 12, false, None, Text),
            ],
        ),
        "UIComponentOnlineLobbyList" => (
            16,
            &[ui_field!("LobbyNameAptTokenSpec", 8, false, None, Text)],
        ),
        "UIComponentOnlinePlayerList" => (
            16,
            &[ui_field!("PlayerNameAptTokenSpec", 8, false, None, Text)],
        ),
        "UIComponentOptions" => (
            112,
            &[
                ui_field!("ControlsToken", 8, true, None, Text),
                ui_field!("SaveFileName", 16, true, None, Text),
                ui_field!("ControlsText", 24, false, None, Repeated(8, &Text)),
                ui_field!("DefaultValues", 32, false, None, Inline("DefaultValues")),
                ui_field!(
                    "CommandList",
                    104,
                    false,
                    None,
                    Repeated(16, &Inline("SingleCommandList"))
                ),
            ],
        ),
        "UIComponentOptionsSettingsAudio" => (
            20,
            &[
                ui_field!("LoadVarQueryReturnToken", 8, false, None, Text),
                ui_field!("DefaultVoipEnable", 16, false, None, Boolean),
                ui_field!("DefaultVoipBoostMicGain", 17, false, None, Boolean),
            ],
        ),
        "UIComponentOptionsSettingsGeneral" => (
            36,
            &[
                ui_field!("LoadVarQueryReturnToken", 8, false, None, Text),
                ui_field!("MinTooltipDelayMS", 16, false, None, Signed),
                ui_field!("MaxTooltipDelayMS", 20, false, None, Signed),
                ui_field!("DefaultTooltipDelayMS", 24, false, None, Signed),
                ui_field!("DefaultMouseSensitivity", 28, false, None, Real),
                ui_field!("DefaultAlternateMouse", 32, false, None, Boolean),
                ui_field!("DefaultShowNews", 33, false, None, Boolean),
                ui_field!("DefaultShowAds", 34, false, None, Boolean),
            ],
        ),
        "UIComponentOptionsSettingsGraphics" => (
            212,
            &[
                ui_field!("LoadVarQueryReturnToken", 8, false, None, Text),
                ui_field!("DefaultBrightness", 16, false, None, Real),
                ui_field!("PresetsAptTokenSpec", 20, false, None, Text),
                ui_field!("PresetsStrLookupSpec", 28, false, None, Text),
                ui_field!("ResolutionAptTokenSpec", 36, false, None, Text),
                ui_field!("AntialiasingAptTokenSpec", 44, false, None, Text),
                ui_field!("AntialiasingOffStrLookupSpec", 52, false, None, Text),
                ui_field!("AntialiasingOnStrLookupSpec", 60, false, None, Text),
                ui_field!("TerrainDetailAptTokenSpec", 68, false, None, Text),
                ui_field!("TerrainDetailStrLookupSpec", 76, false, None, Text),
                ui_field!("WaterDetailAptTokenSpec", 84, false, None, Text),
                ui_field!("WaterDetailStrLookupSpec", 92, false, None, Text),
                ui_field!("ModelDetailAptTokenSpec", 100, false, None, Text),
                ui_field!("ModelDetailStrLookupSpec", 108, false, None, Text),
                ui_field!("TextureQualityAptTokenSpec", 116, false, None, Text),
                ui_field!("TextureQualityStrLookupSpec", 124, false, None, Text),
                ui_field!("ShaderDetailAptTokenSpec", 132, false, None, Text),
                ui_field!("ShaderDetailStrLookupSpec", 140, false, None, Text),
                ui_field!("VFXDetailAptTokenSpec", 148, false, None, Text),
                ui_field!("VFXDetailStrLookupSpec", 156, false, None, Text),
                ui_field!("ShadowDetailAptTokenSpec", 164, false, None, Text),
                ui_field!("ShadowDetailStrLookupSpec", 172, false, None, Text),
                ui_field!("AnimationDetailAptTokenSpec", 180, false, None, Text),
                ui_field!("AnimationDetailStrLookupSpec", 188, false, None, Text),
                ui_field!("DecalDetailAptTokenSpec", 196, false, None, Text),
                ui_field!("DecalDetailStrLookupSpec", 204, false, None, Text),
            ],
        ),
        "UIComponentOptionsSettingsHotkeys" => (
            16,
            &[ui_field!("LoadVarQueryReturnToken", 8, false, None, Text)],
        ),
        "UIComponentOptionsSettingsNetwork" => (
            16,
            &[ui_field!("LoadVarQueryReturnToken", 8, false, None, Text)],
        ),
        "UIComponentPcSaveLoad" => (
            40,
            &[
                ui_field!("MapPrefix", 8, false, None, Text),
                ui_field!("DescriptionPrefix", 16, false, None, Text),
                ui_field!("TimePrefix", 24, false, None, Text),
                ui_field!("DatePrefix", 32, false, None, Text),
            ],
        ),
        "UIComponentPlayerPowers" => (
            12,
            &[ui_field!(
                "ClockImage",
                8,
                true,
                None,
                Weak("PackedTextureImage")
            )],
        ),
        "UIComponentPlayerTechStore" => (
            32,
            &[
                ui_field!("HighlightButtonSoundName", 8, true, None, Text),
                ui_field!("SelectButtonSoundName", 16, true, None, Text),
                ui_field!("CancelButtonSoundName", 24, true, None, Text),
            ],
        ),
        "UIComponentPostGame" => (
            32,
            &[
                ui_field!(
                    "AlliesFactionImage",
                    8,
                    true,
                    None,
                    Strong("PackedTextureImage")
                ),
                ui_field!(
                    "SovietFactionImage",
                    12,
                    true,
                    None,
                    Strong("PackedTextureImage")
                ),
                ui_field!(
                    "JapanFactionImage",
                    16,
                    true,
                    None,
                    Strong("PackedTextureImage")
                ),
                ui_field!("DefaultImage", 20, true, None, Strong("PackedTextureImage")),
                ui_field!(
                    "PostGameStat",
                    24,
                    false,
                    None,
                    Repeated(12, &Inline("PostGameStatData"))
                ),
            ],
        ),
        "UIComponentProfileManager" => (
            16,
            &[ui_field!("ProfileListLabelPrefix", 8, false, None, Text)],
        ),
        "UIComponentRadar" => (
            44,
            &[
                ui_field!("RadarWidth", 8, true, Some("128"), Signed),
                ui_field!("RadarHeight", 12, true, Some("128"), Signed),
                ui_field!("OrientationArrowSize", 16, true, None, Real),
                ui_field!(
                    "OrientationArrowImage",
                    20,
                    true,
                    None,
                    Weak("PackedTextureImage")
                ),
                ui_field!("StatusTextInfiltration", 24, true, None, Text),
                ui_field!("LeftStickMultiplier", 32, true, Some("0.035"), Real),
                ui_field!("RightStickMultiplier", 36, true, Some("350.0"), Real),
                ui_field!("LeftTriggerTolerance", 40, true, Some("900"), Signed),
            ],
        ),
        "UIComponentSaveLoad" => (16, &[ui_field!("FileToken", 8, true, None, Text)]),
        "UIComponentShell" => (
            28,
            &[
                ui_field!("LoadMusic", 8, true, None, Text),
                ui_field!("LogoRenderImage", 16, true, None, Text),
                ui_field!("LogoTexture", 24, true, None, Strong("Texture")),
            ],
        ),
        "UIComponentShellMultiplayer" => (
            72,
            &[
                ui_field!("OfficialMapList", 8, false, None, Inline("MapListType")),
                ui_field!("MatchOptionsRankedLabelSpec", 16, false, None, Text),
                ui_field!("MatchOptionsMatchTypeLabelSpec", 24, false, None, Text),
                ui_field!("MatchOptionsPlayersLabelSpec", 32, false, None, Text),
                ui_field!("AnyOptionLabelSpec", 40, false, None, Text),
                ui_field!("TACLineLabelSpec", 48, false, None, Text),
                ui_field!(
                    "GameType",
                    56,
                    false,
                    None,
                    Repeated(16, &Inline("GameplayTypeSetting"))
                ),
                ui_field!(
                    "RankedOption",
                    64,
                    false,
                    None,
                    Repeated(12, &Inline("RankedOptionSetting"))
                ),
            ],
        ),
        "UIComponentSpecialPowers" => (
            20,
            &[
                ui_field!("PowerTimerImage", 8, true, None, Weak("PackedTextureImage")),
                ui_field!("PowerTimerFlashImageName", 12, true, None, Text),
            ],
        ),
        "UIComponentStats" => (
            32,
            &[
                ui_field!("PlayerToken", 8, true, None, Text),
                ui_field!("TitleToken", 16, true, None, Text),
                ui_field!("DataToken", 24, true, None, Text),
            ],
        ),
        "UIComponentSUDs" => (
            12,
            &[ui_field!(
                "TechPointAwardedSound",
                8,
                true,
                None,
                StrongPointer("BaseAudioEventInfo")
            )],
        ),
        "UIComponentTicker" => (
            40,
            &[
                ui_field!("CustomRenderName", 8, true, None, Text),
                ui_field!("NumberOfTickerDisplays", 16, true, None, Signed),
                ui_field!("ScrollSpeedPixelPerSec", 20, true, None, Real),
                ui_field!("NumberOfPixelsBetweenHeadline", 24, true, None, Real),
                ui_field!("TickerColor", 28, false, None, Inline("Color4")),
                ui_field!(
                    "TickerGroup",
                    32,
                    false,
                    None,
                    Repeated(16, &Inline("UIComponentTickerGroup"))
                ),
            ],
        ),
        "UIComponentTickerGroup" => (
            16,
            &[
                ui_field!("Name", 0, true, None, Text),
                ui_field!("HTTPAddress", 8, true, None, Text),
            ],
        ),
        "UIComponentVictoryDefeat" => (
            24,
            &[
                ui_field!("PostEndGameMusic", 8, false, None, Text),
                ui_field!("EndGameMusicAlias", 16, false, None, Repeated(8, &Text)),
            ],
        ),
        "UIComponentVoiceChat" => (
            12,
            &[ui_field!(
                "TalkHotKeySlot",
                8,
                true,
                None,
                Strong("HotKeySlot")
            )],
        ),
        "Vector2" => (8, &[]),
        "UIComponentAutoMatch"
        | "UIComponentCampaign"
        | "UIComponentCheats"
        | "UIComponentCredits"
        | "UIComponentDisconnect"
        | "UIComponentGameSetup"
        | "UIComponentInGameShowHide"
        | "UIComponentInputBridge"
        | "UIComponentJoypadTactical"
        | "UIComponentLoadScreen"
        | "UIComponentMessage"
        | "UIComponentMode"
        | "UIComponentMouse"
        | "UIComponentMouseInGame"
        | "UIComponentMouseTactical"
        | "UIComponentObjectAction"
        | "UIComponentOnlineLogin"
        | "UIComponentPause"
        | "UIComponentPlasmaInterface"
        | "UIComponentShellVisibility"
        | "UIComponentSound"
        | "UIComponentTutorial" => (8, &[]),
        _ => return None,
    };
    Some(UiLayout { size, fields })
}
