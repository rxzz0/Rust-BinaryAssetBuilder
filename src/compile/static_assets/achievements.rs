use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::global_ui::enum_value;
use super::{Marshaled, sage_uint, significant_children, validate_attributes, write_import};
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
    const PERSONALITY: &str = "AIPersonalityDef";
    validate_leaf(node, &["id", PERSONALITY, EVENT_ID], path)?;
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    write_import(
        required(node, PERSONALITY, path)?,
        "AIPersonalityDefinition",
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
