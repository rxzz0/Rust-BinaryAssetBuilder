use super::binary::BinaryData;
use super::global_ui::enum_value;
use super::{Marshaled, finite_number, significant_children, text_value, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn version(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let children: Vec<_> = significant_children(node).collect();
    let [version] = children.as_slice() else {
        return Err(invalid(path, "expected one Version element"));
    };
    if !version.has_tag_name((SAGE_NS, "Version")) || has_content(*version) {
        return Err(invalid(path, "invalid Version element"));
    }
    validate_attributes(*version, &["Major", "Minor"], path)?;
    let mut data = binary(12, path);
    for (offset, field) in [(4, "Major"), (8, "Minor")] {
        let value = version
            .attribute(field)
            .unwrap_or("0")
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, format!("invalid version {field}")))?;
        data.word(offset, value as u32);
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn online_chat_colors(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const COLORS: &[&str] = &[
        "Default",
        "CurrentRoom",
        "ChatRoom",
        "Game",
        "GameFull",
        "GameCRCMismatch",
        "PlayerNormal",
        "PlayerBuddy",
        "PlayerOwner",
        "OfflinePlayerBuddy",
        "PlayerSelf",
        "PlayerIgnored",
        "OfflinePlayerIgnored",
        "ChatNormal",
        "ChatEmote",
        "ChatOwner",
        "ChatOwnerEmote",
        "ChatPrivate",
        "ChatPrivateEmote",
        "ChatPrivateOwner",
        "ChatPrivateOwnerEmote",
        "ChatBuddy",
        "ChatSelf",
        "AcceptTrue",
        "AcceptFalse",
        "MapSelected",
        "MapUnselected",
        "MessageOfTheDay",
        "MessageOfTheDayHeading",
    ];
    validate_attributes(node, &["id"], path)?;
    let mut data = binary(4 + COLORS.len() * 4, path);
    let mut seen = 0u32;
    for child in significant_children(node) {
        let tag = child.tag_name().name();
        let index = COLORS
            .iter()
            .position(|&name| name == tag)
            .filter(|&index| {
                child.tag_name().namespace() == Some(SAGE_NS) && seen & (1 << index) == 0
            })
            .ok_or_else(|| invalid(path, format!("unexpected OnlineChatColors element {tag}")))?;
        seen |= 1 << index;
        validate_attributes(child, &["R", "G", "B", "A"], path)?;
        if has_content(child) {
            return Err(invalid(path, format!("unexpected {tag} content")));
        }
        for (component, name) in ["B", "G", "R", "A"].into_iter().enumerate() {
            data.payload[4 + index * 4 + component] = child
                .attribute(name)
                .unwrap_or(if name == "A" { "255" } else { "0" })
                .trim()
                .parse::<u8>()
                .map_err(|_| invalid(path, format!("invalid {tag} color component {name}")))?;
        }
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn projectile_path(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut scale = None;
    let mut points = Vec::new();
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "ComponentScale")) && scale.is_none() {
            scale = Some(child);
        } else if child.has_tag_name((SAGE_NS, "Point")) {
            points.push(child);
        } else {
            return Err(invalid(path, "unexpected ProjectilePath child"));
        }
    }
    if points.is_empty() {
        return Err(invalid(path, "ProjectilePath needs at least one Point"));
    }
    let mut data = binary(16, path);
    if let Some(scale) = scale {
        let offset = data.allocate(4, 16)?;
        vector4(scale, offset, &mut data)?;
    }
    let offset = data.list(8, points.len(), 16)?;
    for (index, point) in points.into_iter().enumerate() {
        vector4(point, offset + index * 16, &mut data)?;
    }
    Ok(data.finish(Marshaled::default()))
}

fn vector4(node: Node<'_, '_>, offset: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let names = ["x", "y", "z", "w"];
    validate_attributes(node, &names, data.path)?;
    if has_content(node) {
        return Err(invalid(data.path, "unexpected vector content"));
    }
    for (index, field) in names.into_iter().enumerate() {
        data.word(
            offset + index * 4,
            finite_number(node.attribute(field).unwrap_or("0"), field, data.path)?.to_bits(),
        );
    }
    Ok(())
}

pub(super) fn language(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let separators = children
        .next()
        .filter(|n| n.has_tag_name((SAGE_NS, "Separators")))
        .ok_or_else(|| invalid(path, "expected Language Separators"))?;
    let font_name = children
        .next()
        .filter(|n| n.has_tag_name((SAGE_NS, "UnicodeFontName")))
        .ok_or_else(|| invalid(path, "expected UnicodeFontName"))?;
    let mut data = binary(44, path);
    validate_attributes(
        separators,
        &["Thousand", "Decimal", "TimeMinuteToSecond"],
        path,
    )?;
    if has_content(separators) {
        return Err(invalid(path, "unexpected separators content"));
    }
    for (field, default, offset) in [
        ("Thousand", ",", 4),
        ("Decimal", ".", 12),
        ("TimeMinuteToSecond", ":", 20),
    ] {
        data.string(offset, separators.attribute(field).unwrap_or(default))?;
    }
    data.string(28, text_value(font_name, path)?)?;
    let fonts: Vec<_> = children.collect();
    let start = data.list(36, fonts.len(), 20)?;
    for (index, font) in fonts.into_iter().enumerate() {
        validate_attributes(font, &[], path)?;
        let children: Vec<_> = significant_children(font).collect();
        let [title, desc] = children.as_slice() else {
            return Err(invalid(path, "expected FontTitle and Font"));
        };
        if !font.has_tag_name((SAGE_NS, "FontInfo"))
            || !title.has_tag_name((SAGE_NS, "FontTitle"))
            || !desc.has_tag_name((SAGE_NS, "Font"))
            || has_content(*desc)
        {
            return Err(invalid(path, "unexpected FontInfo content"));
        }
        let offset = start + 20 * index;
        data.word(
            offset,
            enum_value(
                text_value(*title, path)?,
                &[
                    "DrawableCaptionFont",
                    "DefaultWindowFont",
                    "DefaultDisplayStringFont",
                    "TooltipFontName",
                    "NativeDebugDisplay",
                ],
                path,
            )?,
        );
        validate_attributes(*desc, &["Name", "Size", "Bold"], path)?;
        data.string(offset + 4, required(*desc, "Name", path)?)?;
        let size = required(*desc, "Size", path)?
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, "invalid font Size"))?;
        data.word(offset + 12, size);
        data.payload[offset + 16] = match desc.attribute("Bold").unwrap_or("false") {
            "true" | "1" => 1,
            "false" | "0" => 0,
            _ => return Err(invalid(path, "invalid font Bold")),
        };
    }
    Ok(data.finish(Marshaled::default()))
}

fn binary(size: usize, path: &Path) -> BinaryData<'_> {
    BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    }
}

pub(super) fn registered_object_set(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const SET_IDS: &[&str] = &[
        "PLAYER_EVERYTHING",
        "GARRISONS",
        "NEUTRAL_TECH",
        "TIBERIUM_FIELDS",
        "ORE_MINES",
        "SIGMA_FIELDS",
        "GLOBAL_HARVESTERS",
        "GLOBAL_TIBERIUM_REFINERIES",
        "GLOBAL_ORE_REFINERIES",
        "GLOBAL_SIGMA_REFINERIES",
        "STRUCTURES",
        "FACTION_STRUCTURES",
        "PLAYER_NEUTRAL_TECH",
        "UNITS",
        "BEACONS",
        "UNITS_AND_STRUCTURES",
        "BASE_DEFENSES_AND_GARRISONS",
        "MOBILE_GROUND_UNITS",
        "MCVS",
        "DOZERS",
        "HARVESTERS",
        "SURVEYORS",
        "OUTPOSTS",
        "INFANTRY",
        "VEHICLES",
        "SHIPS",
        "AIRCRAFT",
        "UNPACKS_INTO_BUILDING_UNITS",
        "HORDES",
        "HORDE_MEMBERS",
        "ENGINEERS",
        "ENEMY_ENGINEERS",
        "VICTORY_CONDITION_STRUCTURES",
        "CONYARDS",
        "FACTORIES",
        "UNIT_FACTORIES",
        "RADARS",
        "TECH_CENTERS",
        "POWER_PLANTS",
        "REFINERIES",
        "REFINERIES_AND_REFINERY_EGGS",
        "TIBERIUM_REFINERIES",
        "ORE_REFINERIES",
        "SIGMA_REFINERIES",
        "SILOS",
        "FARMS",
        "BASE_DEFENSES",
        "ENEMY_BASE_DEFENSES",
        "AI_PLAYER_POWER_UPDATABLE",
        "SKIRMISH_AI_DEFENSE_TARGETS",
        "SKIRMISH_AI_THREATS_TO_DODGE",
        "SKIRMISH_AI_NORMAL_COMBAT_UNITS",
        "ENCLOSURES",
    ];
    validate_attributes(node, &["id", "SetId", "Type"], path)?;
    let mut children = significant_children(node);
    let filter = children
        .next()
        .filter(|n| n.has_tag_name((SAGE_NS, "ObjectFilter")))
        .ok_or_else(|| invalid(path, "RegisteredObjectSet requires an ObjectFilter"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected RegisteredObjectSet child"));
    }
    let mut data = binary(124, path);
    data.word(
        4,
        enum_value(required(node, "SetId", path)?, SET_IDS, path)?,
    );
    data.word(
        8,
        enum_value(required(node, "Type", path)?, &["GLOBAL", "PLAYER"], path)?,
    );
    let mut result = Marshaled::default();
    super::game_object::encode_inline_filter(filter, 12, &mut data, &mut result.reference_types)?;
    Ok(data.finish(result))
}
