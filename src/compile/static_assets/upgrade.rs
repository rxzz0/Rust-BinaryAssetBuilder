use super::binary::BinaryData;
use super::global_ui::enum_value;
use super::{Marshaled, asset_reference, sage_time, significant_children, validate_attributes};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;
use std::path::Path;

const STRINGS: [(&str, usize); 6] = [
    ("DisplayName", 4),
    ("Description", 0xc),
    ("TypeDescription", 0x14),
    ("AcquireHint", 0x1c),
    ("GroupName", 0x68),
    ("IconImage", 0x7c),
];
const REFERENCES: [(&str, &str, usize); 13] = [
    ("ResearchSound", "BaseAudioEventInfo", 0x30),
    ("ResearchCompleteEvaEvent", "EvaEvent", 0x34),
    ("UnitSpecificSound", "BaseAudioEventInfo", 0x38),
    ("UpgradeFX", "FXList", 0x3c),
    ("LocalPlayerGainsUpgradeEvaEvent", "EvaEvent", 0x40),
    ("AlliedPlayerGainsUpgradeEvaEvent", "EvaEvent", 0x44),
    ("EnemyPlayerGainsUpgradeEvaEvent", "EvaEvent", 0x48),
    ("LocalPlayerLosesUpgradeEvaEvent", "EvaEvent", 0x4c),
    ("AlliedPlayerLosesUpgradeEvaEvent", "EvaEvent", 0x50),
    ("EnemyPlayerLosesUpgradeEvaEvent", "EvaEvent", 0x54),
    ("LocalPlayerProductionStartedEvaEvent", "EvaEvent", 0x58),
    ("LocalPlayerBuildOnHoldEvaEvent", "EvaEvent", 0x5c),
    ("LocalPlayerBuildCancelledEvaEvent", "EvaEvent", 0x60),
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let allowed: Vec<_> = [
        "id",
        "Type",
        "BuildTime",
        "BuildCost",
        "UseObjectTemplateForCostDiscount",
        "GroupOrder",
        "SkirmishAIHeuristic",
        "Options",
        "WaypointQueueable",
    ]
    .into_iter()
    .chain(STRINGS.map(|(name, _)| name))
    .chain(REFERENCES.map(|(name, _, _)| name))
    .collect();
    validate_attributes(node, &allowed, path)?;
    let mut binary = BinaryData {
        payload: vec![0; 0x8c],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for (name, offset) in STRINGS {
        if let Some(value) = node.attribute(name) {
            binary.string(offset, value)?;
        }
    }
    let value = |name, default| node.attribute(name).unwrap_or(default);
    binary.word(
        0x24,
        enum_value(value("Type", "PLAYER"), &["PLAYER", "OBJECT"], path)?,
    );
    binary.word(
        0x28,
        sage_time(value("BuildTime", "0s"), "BuildTime", path)?.to_bits(),
    );
    let cost = value("BuildCost", "0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid BuildCost"))?;
    binary.word(0x2c, cost as u32);
    let group = value("GroupOrder", "0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid GroupOrder"))?;
    binary.word(0x70, group);
    binary.word(
        0x74,
        enum_value(
            value("SkirmishAIHeuristic", "INVALID"),
            &[
                "INVALID",
                "IMPORTANT",
                "BOILINGOIL",
                "FACTORY_UNITUNLOCK",
                "FORTRESS",
                "IGNORE",
            ],
            path,
        )?,
    );
    if let Some(options) = node.attribute("Options") {
        binary.word(
            0x78,
            enum_value(
                options,
                &[
                    "DO_NOT_TRANSFER",
                    "PERSISTS_IN_CAMPAIGN",
                    "NO_UPGRADE_DISCOUNT",
                    "OBJECT_UPGRADE_PROJECTED",
                ],
                path,
            )?,
        );
    }
    binary.payload[0x88] = match value("WaypointQueueable", "false").trim() {
        "true" | "1" => 1,
        "false" | "0" => 0,
        _ => return Err(invalid(path, "invalid WaypointQueueable")),
    };
    for (name, kind, offset) in REFERENCES {
        if let Some(value) = node
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
        {
            let id = reference(value, kind, path)?;
            result.reference_types.push(id.type_id);
            result.references.push(id.into());
            binary.word(offset, result.references.len() as u32);
            result.imports.push(offset as u32);
        }
    }
    if let Some(value) = node
        .attribute("UseObjectTemplateForCostDiscount")
        .filter(|value| !value.trim().is_empty())
    {
        let id = reference(value, "GameObject", path)?;
        result.reference_types.push(id.type_id);
        binary.word(0x64, id.instance_id);
    }
    let mut children = significant_children(node);
    if let Some(dependency) = children.next() {
        if !dependency.has_tag_name((SAGE_NS, "GameDependency")) || children.next().is_some() {
            return Err(invalid(path, "unexpected UpgradeTemplate child"));
        }
        super::game_object::children::dependency(
            dependency,
            0x84,
            &mut binary,
            &mut result.reference_types,
        )?;
    }
    if !binary.relocations.is_empty() {
        binary.relocations.push(u32::MAX);
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = binary.payload;
    result.relocations = binary.relocations;
    Ok(result)
}

fn reference(value: &str, kind: &str, path: &Path) -> io::Result<crate::manifest::AssetId> {
    let name = value.split_once('\\').map_or(value, |(name, _)| name);
    asset_reference(name, kind, path)
}
