use super::ai_strategic::flags;
use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::game_object::{
    encode_die_mux, encode_inline_object_filter, weak_id_list, weak_xml_reference,
};
use super::global_ui::enum_value;
use super::{
    Marshaled, required_element, sage_int, sage_time, sage_uint, significant_children,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

const EVENT_ID: &str = "AchievementEventId";

pub(super) fn event(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ACTION_TYPES: &[&str] = &[
        "TRIGGER_EVENT",
        "AWARD_ACHIEVEMENT_IMMEDIATE",
        "AWARD_ACHIEVEMENT_BEFORE_POST_GAME_SHELL",
        "AWARD_ACHIEVEMENT_AFTER_POST_GAME_SHELL",
    ];
    validate_attributes(node, &["id", "EventId"], path)?;
    let actions: Vec<_> = significant_children(node).collect();
    if actions.is_empty() {
        return Err(invalid(path, "AchievementEvent requires an Action"));
    }
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    data.word(
        4,
        sage_uint(node.attribute("EventId").unwrap_or("0"), "EventId", path)?,
    );
    let start = data.list(8, actions.len(), 12)?;
    for (index, action) in actions.into_iter().enumerate() {
        if !action.has_tag_name((SAGE_NS, "Action")) {
            return Err(invalid(path, "expected AchievementEvent Action"));
        }
        validate_leaf(action, &["TargetCount", "Type", "ActionObjId"], path)?;
        let offset = start + index * 12;
        for (field, position) in [("TargetCount", offset), ("ActionObjId", offset + 8)] {
            data.word(
                position,
                sage_uint(action.attribute(field).unwrap_or("0"), field, path)?,
            );
        }
        data.word(
            offset + 4,
            enum_value(
                action.attribute("Type").unwrap_or("TRIGGER_EVENT"),
                ACTION_TYPES,
                path,
            )?,
        );
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn map_name(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", "MapName", EVENT_ID], path)?;
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    let name = node
        .attribute("MapName")
        .ok_or_else(|| invalid(path, "expected MapName attribute"))?;
    data.string(4, name)?;
    data.word(
        12,
        sage_uint(required(node, EVENT_ID, path)?, EVENT_ID, path)?,
    );
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn personality(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    event_mapping(node, path, "AIPersonalityDef", "AIPersonalityDefinition")
}

pub(super) fn trigger(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    event_mapping(node, path, "GameplayConditionData", "GameplayConditionData")
}

fn event_mapping(
    node: Node<'_, '_>,
    path: &Path,
    field: &str,
    target: &str,
) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", field, EVENT_ID], path)?;
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    write_import(
        required(node, field, path)?,
        target,
        4,
        &mut data,
        &mut result,
    )?;
    data.word(
        8,
        sage_uint(required(node, EVENT_ID, path)?, EVENT_ID, path)?,
    );
    Ok(data.finish(result))
}

pub(super) fn condition(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const HEADER_SIZE: usize = 44;
    const TIME: &str = "TimeReq";
    const CONDITION: &str = "GameplayConditionBitFlags";
    const MODE: &str = "AllowedGameTypeBitFlags";
    const DIFFICULTY: &str = "GameDifficultyBitFlags";
    validate_attributes(node, &["id", TIME, CONDITION, MODE, DIFFICULTY], path)?;
    let mut data = BinaryData {
        payload: vec![0; HEADER_SIZE],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let masks: &[(&str, &str, usize, &[&str])] = &[
        (CONDITION, "", 4, &["KILL", "CAPTURE", "UPGRADE"]),
        (
            MODE,
            "SKIRMISH RANKED_ONLINE",
            12,
            &["CAMPAIGN", "SKIRMISH", "ONLINE", "RANKED_ONLINE"],
        ),
        (
            DIFFICULTY,
            "EASY NORMAL HARD BRUTAL",
            16,
            &["EASY", "NORMAL", "HARD", "BRUTAL"],
        ),
    ];
    for &(field, default, offset, values) in masks {
        data.word(
            offset,
            flags(
                node.attribute(field).unwrap_or(default),
                values,
                field,
                path,
            )?,
        );
    }
    if let Some(value) = node.attribute(TIME) {
        data.optional_word(8, sage_time(value, TIME, path)?.to_bits())?;
    }

    let mut children = significant_children(node).peekable();
    let primary = required_element(&mut children, "PrimaryObject", path)?;
    encode_condition(primary, 20, false, &mut data, &mut result.reference_types)?;
    for (name, pointer, upgrade, size) in [
        ("RequiredObjectCondition", 36, false, 16),
        ("RequiredUpgradeCondition", 40, true, 20),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            let start = data.allocate(pointer, size)?;
            encode_condition(
                child,
                start,
                upgrade,
                &mut data,
                &mut result.reference_types,
            )?;
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected GameplayConditionData child"));
    }
    // EA emits the header's relocations before the nested records' relocations.
    data.relocations
        .sort_by_key(|&offset| offset as usize >= HEADER_SIZE);
    Ok(data.finish(result))
}

fn encode_condition(
    node: Node<'_, '_>,
    start: usize,
    upgrade: bool,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<()> {
    let path = data.path;
    validate_attributes(node, &["Count", "Unique"], path)?;
    data.word(
        start,
        sage_int(node.attribute("Count").unwrap_or("1"), "Count", path)?,
    );
    data.payload[start + 8] = boolean(node, "Unique", false, path)?;
    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")));
    let mut upgrade_types = Vec::new();
    if upgrade {
        let mut upgrades = Vec::new();
        while let Some(child) =
            children.next_if(|child| child.has_tag_name((SAGE_NS, "UpgradeRequired")))
        {
            upgrades.push(weak_xml_reference(
                child,
                "UpgradeTemplate",
                data,
                &mut upgrade_types,
            )?);
        }
        weak_id_list(data, start + 12, upgrades)?;
    } else if let Some(mux) = children.next_if(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
    {
        const DIE_MUX_SIZE: usize = 44;
        let record = data.allocate(start + 12, DIE_MUX_SIZE)?;
        let first = data.relocations.len();
        encode_die_mux(mux, record, data)?;
        data.relocations[first..].sort_by_key(|&offset| offset as usize >= record + DIE_MUX_SIZE);
    }
    if let Some(filter) = filter {
        let record = data.allocate(start + 4, 112)?;
        encode_inline_object_filter(filter, record, data, reference_types)?;
    }
    reference_types.extend(upgrade_types);
    if children.next().is_some() {
        return Err(invalid(
            path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    Ok(())
}
