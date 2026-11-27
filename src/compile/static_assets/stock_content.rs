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
    Time,
    Velocity,
    Hash,
    KindOf,
    Percentage,
    Faction,
    Enum(&'static [&'static str]),
    EnumWithInvalid(&'static [&'static str]),
    EnumPointer(&'static [&'static str]),
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

pub(super) fn simple_button_type_hash(name: &str) -> Option<u32> {
    Some(match name.strip_prefix("UIMouseTactical")? {
        "AttackMoveButton" => 0x5656_3ed8,
        "CoOpAIButtonsAssaultButton" => 0xe640_a7f2,
        "CoOpAIButtonsHelpButton" => 0xbc5a_9f70,
        "CoOpAIButtonsHoldObjectButton" => 0x3432_f9f7,
        "CoOpAIButtonsHoldPositionButton" => 0x7390_ffc7,
        "CoOpAIButtonsNormalButton" => 0x11f9_6319,
        "DeactivateMissionHotSpotsButton" => 0xeaa8_3cd0,
        "FastForwardButton" => 0xba95_a6f2,
        "ForceAttackButton" => 0x089a_2d65,
        "ForceAttackMoveButton" => 0xf9dd_687d,
        "ForceMoveButton" => 0x99e2_501f,
        "OpenAdvancedCommandsButton" => 0x8d93_4671,
        "PauseButton" => 0x3c06_fac3,
        "PlanningModeButton" => 0xee0d_a44f,
        "ReverseMoveButton" => 0x6f0d_7455,
        "SelectionRefinementTab" => 0xf154_81f9,
        "SellModeButton" => 0xb2d8_1257,
        "StopButton" => 0xe76f_f3ab,
        "ThreatMeter" => 0x1c30_f868,
        "ToggleRepairModeButton" => 0x8752_45f7,
        "UnitIntroButton" => 0xf0b7_2ba3,
        "WaypointModeButton" => 0xad5f_905f,
        _ => return None,
    })
}

pub(super) fn ui_display(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let name = node.tag_name().name();
    let layout_name = if simple_button_type_hash(name).is_some() {
        "UIMouseSimpleFixedButton"
    } else {
        name
    };
    let layout = require_ui_layout(layout_name, path)?;
    let mut data = BinaryData {
        payload: vec![0; layout.size],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    write_ui_layout(node, name, layout, 0, &mut data, &mut result)?;
    if matches!(name, "UIMouseVoiceChat" | "UIMouseTacticalQuickChatDisplay") {
        data.relocations.sort_unstable();
    }
    Ok(data.finish(result))
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
    ui_layout(name).ok_or_else(|| invalid(path, format!("unsupported UI layout {name}")))
}

fn write_ui_layout(
    node: Node<'_, '_>,
    name: &str,
    layout: UiLayout,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let is_asset = offset == 0;
    let required_attributes: &[&str] = match name {
        "UIMouseTacticalCommandCommunicateWithAllies" => &[
            "PlaceUnattachedBeaconCursor",
            "PlaceAttachedBeaconCursor",
            "PingRadarCursor",
        ],
        "InfiltratorEffectSettings" => &["Effect"],
        "AIBeaconSettings" => &["Type"],
        "SellFactionCursorSet" => &["Faction"],
        "HUDElementHotKeyMapping" => &["ElementId", "HotKeySlot"],
        "UIQuickChatMessage" => &["MessageText"],
        "CommandButtonFactionSettings" => &["Faction"],
        "StanceButtonSettings" => &["Stance"],
        "UIDecalCloud" => &[
            "BuildTexture",
            "DefenseTexture",
            "SpecialPowerRestrictionTexture",
        ],
        "UIDecalGrid" => &["WallTexture"],
        "DecalSettings" => &["ValidTexture", "InvalidTexture"],
        "UIWorldBuilderDecalGrid" => &["Texture"],
        "RadarSpecialIcon" => &["Image"],
        "JoypadBuildButtonFactionSettings" => &["Faction", "LockedButtonStateData"],
        "BuildQueueTypeSettings" => &["QueueType"],
        "UIMouseTacticalQuickChatDisplay" => &["CloseKey"],
        "QuickChatMenuItem" | "QuickChatCloseItem" => &["Title"],
        "QuickChatMessageReference" => &["MessageRef"],
        _ => &[],
    };
    for attribute in required_attributes {
        required(node, attribute, data.path)?;
    }
    match name {
        "JoypadCommandBarTemplateButtonSlot" => {
            required(node, "Ordinal", data.path)?;
            required(node, "Button", data.path)?;
        }
        "InGameUISimpleHelpTemplate" => {
            required(node, "Title", data.path)?;
        }
        "ObjectFilter" => {
            return super::game_object::encode_inline_object_filter(
                node,
                offset,
                data,
                &mut result.reference_types,
            )
            .map(|_| ());
        }
        "Color3" | "Color4" => {
            let color = if name == "Color3" {
                super::color(node, &["B", "G", "R"], 255, data.path)?
            } else {
                super::color4(node, data.path)?
            };
            data.word(offset, color);
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
        if is_asset && attribute.namespace().is_none() && attribute.name() == "id" {
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
                if name == "UIMouseTacticalGroupSelection" && children.len() > 10 {
                    return Err(invalid(
                        data.path,
                        "UIMouseTacticalGroupSelection accepts at most ten groups",
                    ));
                }
                if field.name == "MenuItem"
                    && ((name == "UIMouseTacticalQuickChatDisplay" && children.is_empty())
                        || children.len() > 9)
                {
                    return Err(invalid(
                        data.path,
                        format!("{name} has an invalid menu item count"),
                    ));
                }
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
            } else if (is_asset
                || matches!(
                    name,
                    "SellFactionCursorSet"
                        | "BuildQueueTypeSettings"
                        | "CommandButtonFactionSettings"
                        | "DecalSettings"
                        | "RadarSpecialIcon"
                        | "StanceButtonSettings"
                ))
                && matches!(field.codec, UiCodec::Inline(_))
            {
                return Err(invalid(
                    data.path,
                    format!("{name} requires {}", field.name),
                ));
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

    let parse_error = || invalid(data.path, format!("invalid UI value {value:?}"));
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
        UiCodec::Time => data.word(
            offset,
            super::sage_time(value, "UI time", data.path)?.to_bits(),
        ),
        UiCodec::Velocity => data.word(
            offset,
            (super::finite_number(value, "UI velocity", data.path)? * 0.2).to_bits(),
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
        UiCodec::Enum(variants)
        | UiCodec::EnumWithInvalid(variants)
        | UiCodec::EnumPointer(variants) => {
            let index = variants
                .iter()
                .position(|&variant| variant == value.trim())
                .ok_or_else(parse_error)?;
            let bias = u32::from(matches!(codec, UiCodec::EnumWithInvalid(_)));
            let value = (index as u32).wrapping_sub(bias);
            if matches!(codec, UiCodec::EnumPointer(_)) {
                data.optional_word(offset, value)?;
            } else {
                data.word(offset, value);
            }
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
const MOUSE_HUD_FIXED_ELEMENT_IDS: [&str; 36] = [
    "Invalid",
    "EnergyMeter",
    "Resources",
    "SellButton",
    "RepairButton",
    "PauseButton",
    "Radar",
    "SelectionRefinementTab",
    "MainStructuresTab",
    "OtherStructuresTab",
    "InfantryTab",
    "VehicleTab",
    "AircraftTab",
    "WatercraftTab",
    "AttackMoveButton",
    "ForceAttackButton",
    "ForceMoveButton",
    "ReverseMoveButton",
    "ForceAttackMoveButton",
    "StopButton",
    "PlanningModeButton",
    "WaypointModeButton",
    "AggressiveStanceButton",
    "GuardStanceButton",
    "HoldFireStanceButton",
    "HoldGroundStanceButton",
    "OpenAdvancedCommandsButton",
    "ThreatMeter",
    "OpenPlayerTechStoreButton",
    "CoOpAINormalOrderButton",
    "CoOpAIAssaultOrderButton",
    "CoOpAIHoldObjectOrderButton",
    "CoOpAIHoldPositionOrderButton",
    "DeactivateHotSpotButton",
    "UnitIntroButton",
    "CoOpAIOrderPanel",
];
fn ui_layout(name: &str) -> Option<UiLayout> {
    use UiCodec::*;
    let (size, fields): (_, &[_]) = match name {
        "UIMouseTacticalBuildQueueTab" | "UIMouseTacticalBuildQueueTypeTab" => (
            12,
            &[ui_field!(
                "QueueTypeSettings",
                4,
                false,
                None,
                Repeated(36, &Inline("BuildQueueTypeSettings"))
            )],
        ),
        "BuildQueueTypeSettings" => (
            36,
            &[
                ui_field!(
                    "QueueType",
                    0,
                    true,
                    None,
                    EnumWithInvalid(&[
                        "INVALID",
                        "MAIN_STRUCTURE",
                        "OTHER_STRUCTURE",
                        "INFANTRY",
                        "VEHICLE",
                        "AIRCRAFT",
                        "UPGRADE",
                        "WATERCRAFT"
                    ])
                ),
                ui_field!(
                    "MouseOverHelp",
                    4,
                    false,
                    None,
                    Inline("InGameUISimpleHelpTemplate")
                ),
            ],
        ),
        "QuickChatMessageReference" => (
            4,
            &[ui_field!(
                "MessageRef",
                0,
                true,
                None,
                Weak("UIQuickChatMessage")
            )],
        ),
        "QuickChatMessageGroup" => (
            12,
            &[
                ui_field!("Faction", 0, true, Some("Neutral"), Faction),
                ui_field!(
                    "ChatMessage",
                    4,
                    false,
                    None,
                    Repeated(4, &Inline("QuickChatMessageReference"))
                ),
            ],
        ),
        "QuickChatCloseItem" => (8, &[ui_field!("Title", 0, true, None, Text)]),
        "QuickChatMenuItem" => (
            32,
            &[
                ui_field!("Title", 0, true, None, Text),
                ui_field!(
                    "ChatDestination",
                    8,
                    true,
                    None,
                    EnumPointer(&["Allies", "Everyone"])
                ),
                ui_field!(
                    "ChatMessageGroup",
                    12,
                    false,
                    None,
                    Repeated(12, &Inline("QuickChatMessageGroup"))
                ),
                ui_field!(
                    "MenuItem",
                    20,
                    false,
                    None,
                    Repeated(32, &Inline("QuickChatMenuItem"))
                ),
                ui_field!(
                    "CloseMenuItem",
                    28,
                    false,
                    None,
                    Pointer("QuickChatCloseItem")
                ),
            ],
        ),
        "UIMouseTacticalQuickChatDisplay" => (
            28,
            &[
                ui_field!("CloseKey", 4, true, None, Strong("HotKeySlot")),
                ui_field!("DelayDuration", 8, true, Some("0s"), Time),
                ui_field!(
                    "MenuItem",
                    12,
                    false,
                    None,
                    Repeated(32, &Inline("QuickChatMenuItem"))
                ),
                ui_field!(
                    "CloseMenuItem",
                    20,
                    false,
                    None,
                    Inline("QuickChatCloseItem")
                ),
            ],
        ),
        "UIMouseTacticalExitContainerButton" => (
            20,
            &[
                ui_field!("HelpTitleFormat", 4, true, None, Text),
                ui_field!("HelpDescription", 12, true, None, Text),
            ],
        ),
        "StanceButtonSettings" => (
            36,
            &[
                ui_field!(
                    "Stance",
                    0,
                    true,
                    None,
                    Enum(&["GUARD", "AGGRESSIVE", "HOLD_POSITION", "HOLD_FIRE"])
                ),
                ui_field!(
                    "MouseOverHelp",
                    4,
                    false,
                    None,
                    Inline("InGameUISimpleHelpTemplate")
                ),
            ],
        ),
        "UIMouseTacticalSetStanceButton" => (
            12,
            &[ui_field!(
                "StanceSettings",
                4,
                false,
                None,
                Repeated(36, &Inline("StanceButtonSettings"))
            )],
        ),
        "CommandButtonFactionSettings" => (
            12,
            &[
                ui_field!("Faction", 0, true, None, Faction),
                ui_field!("LockedImage", 4, true, None, Strong("PackedTextureImage")),
                ui_field!("ImageTintColor", 8, false, None, Inline("Color3")),
            ],
        ),
        "UICommandButton" => (
            12,
            &[ui_field!(
                "FactionSettings",
                4,
                false,
                None,
                Repeated(12, &Inline("CommandButtonFactionSettings"))
            )],
        ),
        "UIDecalCloud" => (
            16,
            &[
                ui_field!("BuildTexture", 4, true, None, Strong("Texture")),
                ui_field!("DefenseTexture", 8, true, None, Strong("Texture")),
                ui_field!(
                    "SpecialPowerRestrictionTexture",
                    12,
                    true,
                    None,
                    Strong("Texture")
                ),
            ],
        ),
        "DecalSettings" => (
            20,
            &[
                ui_field!("ValidTexture", 0, true, None, Strong("Texture")),
                ui_field!("InvalidTexture", 4, true, None, Strong("Texture")),
                ui_field!("Opacity", 8, true, Some("1.0"), Real),
                ui_field!("LegalColor", 12, false, None, Inline("Color3")),
                ui_field!("InvalidColor", 16, false, None, Inline("Color3")),
            ],
        ),
        "UIDecalGrid" => (
            56,
            &[
                ui_field!("WallTexture", 4, true, None, Strong("Texture")),
                ui_field!("WallOpacity", 8, true, Some("1.0"), Real),
                ui_field!("WallDecalColor", 12, false, None, Inline("Color3")),
                ui_field!("PrimarySettings", 16, false, None, Inline("DecalSettings")),
                ui_field!(
                    "SecondarySettings",
                    36,
                    false,
                    None,
                    Inline("DecalSettings")
                ),
            ],
        ),
        "UIWorldBuilderDecalGrid" => (
            16,
            &[
                ui_field!("Texture", 4, true, None, Strong("Texture")),
                ui_field!("Opacity", 8, true, Some("1.0"), Real),
                ui_field!("Color", 12, false, None, Inline("Color3")),
            ],
        ),
        "RadarSpecialIcon" => (
            124,
            &[
                ui_field!("Image", 0, true, None, Strong("PackedTextureImage")),
                ui_field!("ImageOffset", 4, false, None, Inline("Vector2")),
                ui_field!("ObjectFilter", 12, false, None, Inline("ObjectFilter")),
            ],
        ),
        "UIRadarObjectsLayer" => (
            24,
            &[
                ui_field!("MinUnitDotSize", 4, true, Some("3"), Unsigned),
                ui_field!("MinStructureDotSize", 8, true, Some("5"), Unsigned),
                ui_field!("NeutralColor", 12, false, None, Inline("Color4")),
                ui_field!(
                    "SpecialIcon",
                    16,
                    false,
                    None,
                    Repeated(124, &Inline("RadarSpecialIcon"))
                ),
            ],
        ),
        "JoypadBuildButtonFactionSettings" => (
            8,
            &[
                ui_field!("Faction", 0, true, None, Faction),
                ui_field!(
                    "LockedButtonStateData",
                    4,
                    true,
                    None,
                    Strong("ButtonSingleStateData")
                ),
            ],
        ),
        "UIJoypadCommandBarButtonBuild" => (
            12,
            &[ui_field!(
                "FactionSettings",
                4,
                false,
                None,
                Repeated(8, &Inline("JoypadBuildButtonFactionSettings"))
            )],
        ),
        "UICoopTinting" => (
            28,
            &[
                ui_field!("FadeInFrames", 4, true, Some("3"), Signed),
                ui_field!("FadeOutFrames", 8, true, Some("3"), Signed),
                ui_field!("SustainFrames", 12, true, Some("10"), Signed),
                ui_field!("TintFrequency", 16, true, Some("1.0"), Real),
                ui_field!("TintAmplitude", 20, true, Some("1.0"), Real),
                ui_field!("TintColor", 24, false, None, Inline("Color3")),
            ],
        ),
        "UIMouseDragSelectionFeedback" => (
            20,
            &[
                ui_field!("BoxEdgeWidth", 4, true, Some("2"), Real),
                ui_field!("LassoWidth", 8, true, Some("2"), Real),
                ui_field!("BoxColor", 12, false, None, Inline("Color4")),
                ui_field!("LassoColor", 16, false, None, Inline("Color4")),
            ],
        ),
        "HUDElementHotKeyMapping" => (
            8,
            &[
                ui_field!(
                    "ElementId",
                    0,
                    true,
                    None,
                    EnumWithInvalid(&MOUSE_HUD_FIXED_ELEMENT_IDS)
                ),
                ui_field!("HotKeySlot", 4, true, None, Strong("HotKeySlot")),
            ],
        ),
        "UIMouseHUDElementHotKeyGate" => (
            12,
            &[ui_field!(
                "ElementHotKeyMapping",
                4,
                false,
                None,
                Repeated(8, &Inline("HUDElementHotKeyMapping"))
            )],
        ),
        "UIMouseTacticalPlayerTechStoreButton" => (
            40,
            &[
                ui_field!(
                    "TechPointAwardedSound",
                    36,
                    true,
                    None,
                    StrongPointer("BaseAudioEventInfo")
                ),
                ui_field!(
                    "MouseOverHelp",
                    4,
                    false,
                    None,
                    Inline("InGameUISimpleHelpTemplate")
                ),
            ],
        ),
        "UIMouseVoiceChat" => (
            20,
            &[
                ui_field!("ToggleModeHotKeySlot", 4, true, None, Strong("HotKeySlot")),
                ui_field!("TalkHotKeySlot", 8, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "ModeButtonHelp",
                    12,
                    false,
                    None,
                    Pointer("InGameUISimpleHelpTemplate")
                ),
                ui_field!(
                    "TalkButtonHelp",
                    16,
                    false,
                    None,
                    Pointer("InGameUISimpleHelpTemplate")
                ),
            ],
        ),
        "UIQuickChatMessage" => (
            16,
            &[
                ui_field!("MessageText", 4, true, None, Text),
                ui_field!("Sound", 12, true, None, Strong("BaseAudioEventInfo")),
            ],
        ),
        "UIMouseTacticalCommandCommunicateWithAllies" => (
            20,
            &[
                ui_field!(
                    "PlaceUnattachedBeaconCursor",
                    4,
                    true,
                    None,
                    Strong("MouseCursor")
                ),
                ui_field!(
                    "PlaceAttachedBeaconCursor",
                    8,
                    true,
                    None,
                    Strong("MouseCursor")
                ),
                ui_field!("PingRadarCursor", 12, true, None, Strong("MouseCursor")),
                ui_field!(
                    "BeaconAttachableObjectFilter",
                    16,
                    false,
                    None,
                    Pointer("ObjectFilter")
                ),
            ],
        ),
        "InfiltratorEffectSettings" => (
            8,
            &[
                ui_field!(
                    "Effect",
                    0,
                    true,
                    None,
                    EnumWithInvalid(&[
                        "INVALID",
                        "RADAR_FREEZE",
                        "DISABLE",
                        "STEAL_MONEY",
                        "VISION",
                        "ENERGY",
                        "RESET",
                        "VISION_UNITS",
                        "PARALYZE",
                        "KILL",
                    ])
                ),
                ui_field!("Cursor", 4, true, None, Strong("MouseCursor")),
            ],
        ),
        "UIMouseTacticalCommandEnter" => (
            20,
            &[
                ui_field!("DefaultCursor", 4, true, None, Strong("MouseCursor")),
                ui_field!("ContestCursor", 8, true, None, Strong("MouseCursor")),
                ui_field!(
                    "InfiltratorEffectTypeSettings",
                    12,
                    false,
                    None,
                    Repeated(8, &Inline("InfiltratorEffectSettings"))
                ),
            ],
        ),
        "AIBeaconSettings" => (
            20,
            &[
                ui_field!(
                    "Type",
                    0,
                    true,
                    None,
                    EnumWithInvalid(&["Invalid", "Assault", "HoldPosition", "HoldObject"])
                ),
                ui_field!(
                    "PlaceUnattachedCursor",
                    4,
                    true,
                    None,
                    Strong("MouseCursor")
                ),
                ui_field!("PlaceAttachedCursor", 8, true, None, Strong("MouseCursor")),
                ui_field!(
                    "AttachableObjectFilter",
                    12,
                    false,
                    None,
                    Pointer("ObjectFilter")
                ),
                ui_field!("CanTargetObjects", 16, true, Some("true"), Boolean),
                ui_field!("CanTargetPosition", 17, true, Some("true"), Boolean),
            ],
        ),
        "UIMouseTacticalCommandPlaceAIBeacon" => (
            12,
            &[ui_field!(
                "BeaconTypeSettings",
                4,
                false,
                None,
                Repeated(20, &Inline("AIBeaconSettings"))
            )],
        ),
        "UIMouseTacticalCommandSelect" => (
            16,
            &[
                ui_field!("Cursor", 4, true, None, Strong("MouseCursor")),
                ui_field!(
                    "TakeUnitsFromAllyHotKeySlot",
                    8,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!(
                    "GiveUnitsToAllyHotKeySlot",
                    12,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
            ],
        ),
        "SellCursorSet" => (
            8,
            &[
                ui_field!("ValidTarget", 0, true, None, Strong("MouseCursor")),
                ui_field!("InvalidTarget", 4, true, None, Strong("MouseCursor")),
            ],
        ),
        "SellFactionCursorSet" => (
            12,
            &[
                ui_field!("Faction", 0, true, None, Faction),
                ui_field!("Cursors", 4, false, None, Inline("SellCursorSet")),
            ],
        ),
        "UIMouseTacticalCommandSell" => (
            20,
            &[
                ui_field!("DefaultCursors", 4, false, None, Inline("SellCursorSet")),
                ui_field!(
                    "FactionCursors",
                    12,
                    false,
                    None,
                    Repeated(12, &Inline("SellFactionCursorSet"))
                ),
            ],
        ),
        "UIMouseTacticalGroupSelectionGroup" => (
            16,
            &[
                ui_field!("CreateHotKeySlot", 0, true, None, Strong("HotKeySlot")),
                ui_field!("AddToHotKeySlot", 4, true, None, Strong("HotKeySlot")),
                ui_field!("SelectHotKeySlot", 8, true, None, Strong("HotKeySlot")),
                ui_field!("ViewHotKeySlot", 12, true, None, Strong("HotKeySlot")),
            ],
        ),
        "UIMouseTacticalGroupSelection" => (
            12,
            &[ui_field!(
                "Group",
                4,
                false,
                None,
                Repeated(16, &Inline("UIMouseTacticalGroupSelectionGroup"))
            )],
        ),
        "UIMouseTacticalLookAtViewBookmark" => (
            8,
            &[
                ui_field!("GoToHotKeySlot", 0, true, None, Strong("HotKeySlot")),
                ui_field!("SaveHotKeySlot", 4, true, None, Strong("HotKeySlot")),
            ],
        ),
        "UIMouseTacticalLookAt" => (
            60,
            &[
                ui_field!("ScrollCursor", 4, true, None, Strong("MouseCursor")),
                ui_field!("ScrollLeftHotKeySlot", 8, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "ScrollRightHotKeySlot",
                    12,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!("ScrollUpHotKeySlot", 16, true, None, Strong("HotKeySlot")),
                ui_field!("ScrollDownHotKeySlot", 20, true, None, Strong("HotKeySlot")),
                ui_field!("RotateLeftHotKeySlot", 24, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "RotateRightHotKeySlot",
                    28,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!("ZoomInHotKeySlot", 32, true, None, Strong("HotKeySlot")),
                ui_field!("ZoomOutHotKeySlot", 36, true, None, Strong("HotKeySlot")),
                ui_field!("KeyScrollFactor", 40, true, Some("100%"), Percentage),
                ui_field!("MouseDragScrollFactor", 44, true, Some("100%"), Percentage),
                ui_field!("MouseEdgeScrollFactor", 48, true, Some("100%"), Percentage),
                ui_field!(
                    "ViewBookmark",
                    52,
                    false,
                    None,
                    Repeated(8, &Inline("UIMouseTacticalLookAtViewBookmark"))
                ),
            ],
        ),
        "UIMouseTacticalMiscHotKeysStanceSettings" => (
            8,
            &[
                ui_field!(
                    "Stance",
                    0,
                    true,
                    None,
                    Enum(&["GUARD", "AGGRESSIVE", "HOLD_POSITION", "HOLD_FIRE"])
                ),
                ui_field!("Slot", 4, true, None, Strong("HotKeySlot")),
            ],
        ),
        "UIMouseTacticalMiscHotKeys" => (
            200,
            &[
                ui_field!("AllCheer", 4, true, None, Strong("HotKeySlot")),
                ui_field!("AttackMove", 8, true, None, Strong("HotKeySlot")),
                ui_field!("CameraReset", 12, true, None, Strong("HotKeySlot")),
                ui_field!("ChatWithAllies", 16, true, None, Strong("HotKeySlot")),
                ui_field!("ChatWithBuddies", 20, true, None, Strong("HotKeySlot")),
                ui_field!("ChatWithEveryone", 24, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "CommunicateWithAlliesMode",
                    28,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!("CreateFormation", 32, true, None, Strong("HotKeySlot")),
                ui_field!("CycleHarvesters", 36, true, None, Strong("HotKeySlot")),
                ui_field!("DeleteBeacon", 40, true, None, Strong("HotKeySlot")),
                ui_field!("ForceAttack", 44, true, None, Strong("HotKeySlot")),
                ui_field!("ForceAttackMove", 48, true, None, Strong("HotKeySlot")),
                ui_field!("ForceMove", 52, true, None, Strong("HotKeySlot")),
                ui_field!("GoToNextBuildQueue", 56, true, None, Strong("HotKeySlot")),
                ui_field!("GoToPriorBuildQueue", 60, true, None, Strong("HotKeySlot")),
                ui_field!("GoToNextSubGroup", 64, true, None, Strong("HotKeySlot")),
                ui_field!("GoToPriorSubGroup", 68, true, None, Strong("HotKeySlot")),
                ui_field!("OpenAdvancedCommands", 72, true, None, Strong("HotKeySlot")),
                ui_field!("OpenLoadGameScreen", 76, true, None, Strong("HotKeySlot")),
                ui_field!("OpenMessenger", 80, true, None, Strong("HotKeySlot")),
                ui_field!("OpenPauseScreen", 84, true, None, Strong("HotKeySlot")),
                ui_field!("OpenPlayerTechStore", 88, true, None, Strong("HotKeySlot")),
                ui_field!("OpenSaveGameScreen", 92, true, None, Strong("HotKeySlot")),
                ui_field!("OpenStatusScreen", 96, true, None, Strong("HotKeySlot")),
                ui_field!("PlaceRallyPoint", 100, true, None, Strong("HotKeySlot")),
                ui_field!("PlanningMode", 104, true, None, Strong("HotKeySlot")),
                ui_field!("PreferSelection", 108, true, None, Strong("HotKeySlot")),
                ui_field!("QuickChatMenu", 112, true, None, Strong("HotKeySlot")),
                ui_field!("ReverseMove", 116, true, None, Strong("HotKeySlot")),
                ui_field!("Scatter", 120, true, None, Strong("HotKeySlot")),
                ui_field!("SelectAll", 124, true, None, Strong("HotKeySlot")),
                ui_field!("SelectMatchingUnits", 128, true, None, Strong("HotKeySlot")),
                ui_field!("SellMode", 132, true, None, Strong("HotKeySlot")),
                ui_field!("ShowAllHealthBars", 136, true, None, Strong("HotKeySlot")),
                ui_field!("Stop", 140, true, None, Strong("HotKeySlot")),
                ui_field!("TelestratorErase", 144, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "TelestratorNextColor",
                    148,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!(
                    "TelestratorNextLineWidth",
                    152,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!(
                    "TelestratorPriorColor",
                    156,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!("TelestratorToggle", 160, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "ToggleFastForwardMode",
                    164,
                    true,
                    None,
                    Strong("HotKeySlot")
                ),
                ui_field!("ToggleHUD", 168, true, None, Strong("HotKeySlot")),
                ui_field!("TogglePowerMode", 172, true, None, Strong("HotKeySlot")),
                ui_field!("ToggleRepairMode", 176, true, None, Strong("HotKeySlot")),
                ui_field!("ViewHomeBase", 180, true, None, Strong("HotKeySlot")),
                ui_field!("ViewLastEvaEvent", 184, true, None, Strong("HotKeySlot")),
                ui_field!("WaypointMode", 188, true, None, Strong("HotKeySlot")),
                ui_field!(
                    "StanceSettings",
                    192,
                    false,
                    None,
                    Repeated(8, &Inline("UIMouseTacticalMiscHotKeysStanceSettings"))
                ),
            ],
        ),
        "UIMouseTacticalPlayerPowerHotKeys" => (
            12,
            &[ui_field!(
                "Slot",
                4,
                false,
                None,
                Repeated(4, &Strong("HotKeySlot"))
            )],
        ),
        "UIMouseTacticalRadarTelestrator" => (
            12,
            &[
                ui_field!("StrokeSustainDuration", 4, true, Some("10s"), Time),
                ui_field!("StrokeFadeDuration", 8, true, Some("2s"), Time),
            ],
        ),
        "UIMouseTacticalTelestrator" => (
            8,
            &[ui_field!(
                "DrawCursor",
                4,
                true,
                None,
                Strong("MouseCursor")
            )],
        ),
        "UIMouseTacticalSelectionDetailsButtonHotKeys" => (
            12,
            &[ui_field!(
                "UnitAbilitySlot",
                4,
                false,
                None,
                Repeated(4, &Strong("HotKeySlot"))
            )],
        ),
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
        "JoypadCommandBarTemplate" => (
            24,
            &[
                ui_field!(
                    "HomeButton",
                    4,
                    true,
                    None,
                    Strong("JoypadCommandBarButtonTemplate")
                ),
                ui_field!(
                    "Slot",
                    8,
                    false,
                    None,
                    Repeated(8, &Inline("JoypadCommandBarTemplateButtonSlot"))
                ),
                ui_field!(
                    "GeneralButton",
                    16,
                    false,
                    None,
                    Repeated(4, &Strong("JoypadCommandBarButtonTemplate"))
                ),
            ],
        ),
        "JoypadCommandBarTemplateButtonSlot" => (
            8,
            &[
                ui_field!("Ordinal", 0, true, None, Unsigned),
                ui_field!(
                    "Button",
                    4,
                    true,
                    None,
                    Strong("JoypadCommandBarButtonTemplate")
                ),
            ],
        ),
        "UIJoypadCommandBarControlGroups"
        | "UIJoypadCommandBarHomogenousGroup"
        | "UIJoypadCommandBarMixedGroup"
        | "UIJoypadCommandBarPlayerPowers"
        | "UIJoypadCommandBarStances"
        | "UIJoypadCommandBarTopMenu" => (
            8,
            &[ui_field!(
                "Template",
                4,
                true,
                None,
                Strong("JoypadCommandBarTemplate")
            )],
        ),
        "UIJoypadCommandBarCoopCommunicator" => (
            8,
            &[ui_field!(
                "AICoopTemplate",
                4,
                true,
                None,
                Strong("JoypadCommandBarTemplate")
            )],
        ),
        "UIJoypadCommandBarSingleUnit" => (
            20,
            &[
                ui_field!(
                    "UnitTemplate",
                    4,
                    true,
                    None,
                    Strong("JoypadCommandBarTemplate")
                ),
                ui_field!(
                    "ProductionStructureTemplate",
                    8,
                    true,
                    None,
                    Strong("JoypadCommandBarTemplate")
                ),
                ui_field!(
                    "NonProductionStructureTemplate",
                    12,
                    true,
                    None,
                    Strong("JoypadCommandBarTemplate")
                ),
                ui_field!(
                    "HotSpotTemplate",
                    16,
                    true,
                    None,
                    Strong("JoypadCommandBarTemplate")
                ),
            ],
        ),
        "UIMouseSimpleFixedButton" => (
            36,
            &[ui_field!(
                "MouseOverHelp",
                4,
                false,
                None,
                Inline("InGameUISimpleHelpTemplate")
            )],
        ),
        "InGameUISimpleHelpTemplate" => (
            32,
            &[
                ui_field!("Title", 0, true, None, Text),
                ui_field!("Description", 8, true, None, Text),
                ui_field!("TypeDescription", 16, true, None, Text),
                ui_field!("Instructions", 24, true, None, Text),
            ],
        ),
        "HeaderTemplate" => (20, &[ui_field!("Font", 4, false, None, Inline("FontDesc"))]),
        "UIFloatingTextDisplay" => (
            32,
            &[
                ui_field!("TimeOut", 4, true, Some("333ms"), Time),
                ui_field!("MoveUpSpeed", 8, true, Some("1.0"), Velocity),
                ui_field!("MoveVanishRate", 12, true, Some("0.1"), Velocity),
                ui_field!("Font", 16, false, None, Inline("FontDesc")),
            ],
        ),
        "UISubtitleDisplay" => (
            36,
            &[
                ui_field!("HoldTime", 4, true, Some("20s"), Time),
                ui_field!("LineCount", 8, true, Some("8"), Signed),
                ui_field!("Font", 12, false, None, Inline("FontDesc")),
                ui_field!("PanelColor", 28, false, None, Inline("Color4")),
                ui_field!("TextColor", 32, false, None, Inline("Color3")),
            ],
        ),
        "Color3" => (4, &[]),
        "UIMessageDisplay" => (
            32,
            &[
                ui_field!("Delay", 4, true, Some("5s"), Time),
                ui_field!("Color1", 8, false, None, Inline("Color3")),
                ui_field!("Color2", 12, false, None, Inline("Color3")),
                ui_field!("Font", 16, false, None, Inline("FontDesc")),
            ],
        ),
        "UITimerDisplay" => (
            64,
            &[
                ui_field!("FlashDuration", 4, true, Some("1s"), Time),
                ui_field!("Position", 8, false, None, Inline("Vector2")),
                ui_field!("FlashColor", 16, false, None, Inline("Color3")),
                ui_field!("NormalFont", 20, false, None, Inline("FontDesc")),
                ui_field!("NormalColor", 36, false, None, Inline("Color3")),
                ui_field!("ReadyFont", 40, false, None, Inline("FontDesc")),
                ui_field!("ReadyColor", 56, false, None, Inline("Color3")),
                ui_field!("Centered", 60, true, Some("false"), Boolean),
            ],
        ),
        "UIMilitaryCaptionDisplay" => (
            56,
            &[
                ui_field!("Delay", 4, true, Some("750ms"), Time),
                ui_field!("Color", 8, false, None, Inline("Color3")),
                ui_field!("Position", 12, false, None, Inline("Vector2")),
                ui_field!("TitleFont", 20, false, None, Inline("FontDesc")),
                ui_field!("Font", 36, false, None, Inline("FontDesc")),
                ui_field!("Centered", 52, true, Some("false"), Boolean),
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
