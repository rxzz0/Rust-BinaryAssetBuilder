use super::ai_strategic::flags;
use super::binary::BinaryData;
use super::game_object::{write_model_conditions, write_status_flags};
use super::global_ui::enum_value;
use super::{
    Marshaled, asset_reference, sage_time, sage_uint, significant_children, text_value,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid};
use crate::hash::fast_hash;
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ATTRIBUTES: &[&str] = &[
        "id",
        "ForbiddenConditions",
        "ForbiddenConditionExceptions",
        "ForbiddenStatus",
        "CamouflageLevel",
        "InvisibilityType",
        "Options",
        "EnteringStealthFX",
        "LeavingStealthFX",
        "HintDetectableStates",
        "NoStealthForAttackWindow",
        "RequiredStatus",
    ];
    const DEFAULT_CONDITIONS: &str = concat!(
        "PREATTACK_A FIRING_A FIRING_OR_PREATTACK_A RELOADING_A USING_WEAPON_A ",
        "PREATTACK_B FIRING_B FIRING_OR_PREATTACK_B RELOADING_B USING_WEAPON_B ",
        "PREATTACK_C FIRING_C FIRING_OR_PREATTACK_C RELOADING_C USING_WEAPON_C"
    );
    const DEFAULT_STATUS: &str = "IS_FIRING_WEAPON IS_AIMING_WEAPON SPECIAL_ABILITY_PACKING_UNPACKING_OR_USING USING_ABILITY";
    const OPTIONS: &[&str] = &[
        "ALLOW_NEAR_TREES",
        "DETECTED_BY_FRIENDLIES",
        "DISCONTINUE_WHEN_REVEALED",
        "UNTOGGLE_HIDDEN_WHEN_LEAVING_STEALTH",
        "CAN_ATTACK_WHILE_ACTIVE",
    ];
    validate_attributes(node, ATTRIBUTES, path)?;
    let mut data = BinaryData {
        payload: vec![0; 240],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for (name, default, offset) in [
        ("ForbiddenConditions", DEFAULT_CONDITIONS, 4),
        ("ForbiddenConditionExceptions", "", 64),
    ] {
        write_model_conditions(node.attribute(name).unwrap_or(default), offset, &mut data)?;
    }
    for (name, default, offset) in [
        ("ForbiddenStatus", DEFAULT_STATUS, 124),
        ("HintDetectableStates", "IS_ATTACKING", 172),
        ("RequiredStatus", "", 204),
    ] {
        write_status_flags(node.attribute(name).unwrap_or(default), offset, &mut data)?;
    }
    data.word(
        152,
        sage_uint(
            node.attribute("CamouflageLevel").unwrap_or("0"),
            "CamouflageLevel",
            path,
        )?,
    );
    data.word(
        156,
        enum_value(
            node.attribute("InvisibilityType").unwrap_or("STEALTH"),
            &["INVALID", "STEALTH", "CAMOUFLAGE"],
            path,
        )?
        .wrapping_sub(1),
    );
    data.word(
        160,
        flags(
            node.attribute("Options").unwrap_or(""),
            OPTIONS,
            "Options",
            path,
        )?,
    );
    const WINDOW: &str = "NoStealthForAttackWindow";
    data.word(
        200,
        sage_time(node.attribute(WINDOW).unwrap_or("0s"), WINDOW, path)?.to_bits(),
    );
    for attribute in node.attributes() {
        let offset = match attribute.name() {
            "EnteringStealthFX" => 164,
            "LeavingStealthFX" => 168,
            _ => continue,
        };
        write_import(attribute.value(), "FXList", offset, &mut data, &mut result)?;
    }
    let upgrades: Vec<_> = significant_children(node).collect();
    let start = data.list(232, upgrades.len(), 4)?;
    for (index, upgrade) in upgrades.into_iter().enumerate() {
        if !upgrade.has_tag_name((SAGE_NS, "IgnoreTreeCheckUpgrade")) {
            return Err(invalid(path, "expected an IgnoreTreeCheckUpgrade"));
        }
        let id = asset_reference(text_value(upgrade, path)?, "UpgradeTemplate", path)?;
        data.word(start + index * 4, id.instance_id);
        result.reference_types.push(fast_hash(b"UpgradeTemplate"));
    }
    Ok(data.finish(result))
}
