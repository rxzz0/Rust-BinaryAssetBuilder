use super::{
    Marshaled, asset_reference, fast_hash, finite_number, percentage, significant_children,
    validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required, windows_1252};
use roxmltree::Node;
use std::io;
use std::path::Path;

enum FieldKind {
    Float,
    Signed,
    Unsigned,
    Percent,
    Time,
    Difficulty,
    EnemySorting,
    Portrait,
}

use FieldKind::*;

// The contiguous fields at byte offsets 20 through 232 in the RA3 PC payload.
const SCALARS: [(&str, &str, FieldKind); 54] = [
    ("SecondsTillTargetsExpire", "15", Float),
    ("DesiredExcessPowerBuffer", "0", Signed),
    ("BaseCompactness", "0.5", Float),
    ("DefaultThreatFindRadius", "500", Float),
    ("UnitBuilderCostEffectivenessWeight", "1", Float),
    ("UnitBuilderMoneyVersusTimePreference", "0.5", Float),
    ("UnitBuilderOverallOffensivePreference", "100%", Percent),
    ("UnitBuilderOverallDefensivePreference", "100%", Percent),
    ("UnitBuilderEnemyTowerWeight", "100%", Percent),
    ("UnitBuilderEnemyNearbyDistance", "800", Float),
    ("UnitBuilderEnemyNearbyMultiplier", "4", Unsigned),
    ("BuildTechEfficiencyThreshold", "0.5", Float),
    ("BuildTechMinPowerAdvantage", "1.1", Float),
    ("BuildExtraFactoryMinTime", "240s", Time),
    ("ResourceNodeSearchRadius", "500", Float),
    ("ResourceNodeValueRemainingForExpansion", "3000", Signed),
    ("RefineryPlacementStartDistanceDelta", "0", Float),
    ("RefineryPlacementIncreaseTickAmount", "60", Float),
    (
        "RefineryPlacementNumberOfIncreaseTicksBeforeGivingUp",
        "3",
        Signed,
    ),
    ("ReactiveDefenseRadius", "1200", Float),
    ("RepairBuildingsAtDifficulty", "", Difficulty),
    (
        "UsePlayerPowersAtDifficulty",
        "EASY MEDIUM HARD BRUTAL",
        Difficulty,
    ),
    ("EmergencyManagerHandleNoPowerAtDifficulty", "", Difficulty),
    ("EmergencyManagerHandleNoIncomeAtDifficulty", "", Difficulty),
    (
        "EmergencyManagerHandleNoConyardAtDifficulty",
        "",
        Difficulty,
    ),
    (
        "EmergencyManagerHandleUnderAttackAtDifficulty",
        "",
        Difficulty,
    ),
    (
        "EmergencyManagerHandleConyardInDangerAtDifficulty",
        "",
        Difficulty,
    ),
    ("ConyardInDangerEmergencyUnderAttackTime", "10s", Time),
    ("ConyardInDangerEmergencyHealthPercent", "50%", Percent),
    ("ConyardInDangerEmergencyEngineerDistance", "200", Float),
    (
        "EnemySortingBrutalDifficulty",
        "NearestFirst_BreakTies",
        EnemySorting,
    ),
    (
        "EnemySortingHardDifficulty",
        "NearestFirst_BreakTies",
        EnemySorting,
    ),
    ("EnemySortingMediumDifficulty", "Flat", EnemySorting),
    (
        "EnemySortingEasyDifficulty",
        "StrongestFirst_AllowTies",
        EnemySorting,
    ),
    ("EnemyResortingTime", "15s", Time),
    ("StructureSaveChanceEasy", "0", Float),
    ("StructureSaveChanceMedium", "0", Float),
    ("StructureSaveChanceHard", "0", Float),
    ("BaseBuilderKeepUnderAttackFlagOnTime", "4s", Time),
    ("BaseBuilderUpdateUnderAttackTime", "200ms", Time),
    ("BaseBuilderUpdateEnemiesInBaseTime", "1s", Time),
    ("MinHarvesters", "0", Unsigned),
    ("MaxStrategicTargets", "32", Signed),
    ("AssaultBeaconRadius", "300", Float),
    ("HoldObjectBeaconRadius", "300", Float),
    ("HoldPositionBeaconRadius", "300", Float),
    ("RedAlertPowerAdvantage", "0.5", Float),
    ("RedAlertIncome", "50", Float),
    ("RedAlertProductionBuildings", "2", Signed),
    ("RedAlertAttackTime", "15s", Time),
    ("MaxWallDestroyTimeToCountAsGoThroughWalls", "1s", Time),
    ("CommanderPortrait", DEFAULT_PORTRAIT, Portrait),
    ("SkirmishTimeTillGreeting", "1.5s", Time),
    ("CoopMonitorUpdateTime", "5s", Time),
];

const FLAGS: [&str; 4] = [
    "SkirmishPersonality",
    "CoopPersonality",
    "CleanupRefineries",
    "CanBuildInAlliedBuildRadii",
];
const PORTRAIT_TYPE: &str = "PackedTextureImage";
const DEFAULT_PORTRAIT: &str = "AI_Portrait_TEMP";

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let allowed: Vec<_> = ["id", "PersonalityType", "PersonalityUIName"]
        .into_iter()
        .chain(FLAGS)
        .chain(SCALARS.iter().map(|(name, _, _)| *name))
        .collect();
    validate_attributes(node, &allowed, path)?;

    let mut children = significant_children(node);
    let filter = children
        .next()
        .ok_or_else(|| invalid(path, "missing AI structure filter"))?;
    if !filter.has_tag_name((SAGE_NS, "CountAsStructureForBuildPurposesFilter"))
        || filter.attributes().len() != 0
        || has_content(filter)
        || children.next().is_some()
    {
        return Err(invalid(
            path,
            "AI personality supports an empty structure filter and scalar settings",
        ));
    }

    let mut payload = vec![0; 512];
    for (index, (name, default, kind)) in SCALARS.iter().enumerate() {
        let value = node.attribute(*name).unwrap_or(default).trim();
        put(
            &mut payload,
            20 + index * 4,
            kind.encode(value, name, path)?,
        );
    }
    put(&mut payload, 388, 3); // ObjectFilterRule::NONE
    for (index, name) in FLAGS.into_iter().enumerate() {
        payload[508 + index] = match node.attribute(name).unwrap_or("false").trim() {
            "true" | "1" => 1,
            "false" | "0" => 0,
            _ => return Err(invalid(path, format!("invalid AI personality {name}"))),
        };
    }

    let personality = required(node, "PersonalityType", path)?;
    let mut relocations = vec![8];
    append_string(&mut payload, 4, personality, path)?;
    if let Some(ui_name) = node.attribute("PersonalityUIName") {
        append_string(&mut payload, 12, ui_name, path)?;
        relocations.push(16);
    }
    relocations.push(u32::MAX);
    let portrait = node
        .attribute("CommanderPortrait")
        .unwrap_or(DEFAULT_PORTRAIT);
    Ok(Marshaled {
        payload,
        relocations,
        reference_types: if portrait.trim().is_empty() {
            Vec::new()
        } else {
            vec![fast_hash(PORTRAIT_TYPE.as_bytes())]
        },
        ..Marshaled::default()
    })
}

impl FieldKind {
    fn encode(&self, value: &str, field: &str, path: &Path) -> io::Result<u32> {
        let error = || invalid(path, format!("invalid AI personality {field}"));
        match self {
            Float => Ok(finite_number(value, field, path)?.to_bits()),
            Signed => value
                .parse::<i32>()
                .map(|number| number as u32)
                .map_err(|_| error()),
            Unsigned => value.parse::<u32>().map_err(|_| error()),
            Percent => Ok(percentage(value, field, path)?.to_bits()),
            Time => {
                let (number, factor) = if let Some(number) = value.strip_suffix("ms") {
                    (number, 0.001)
                } else {
                    (value.strip_suffix('s').ok_or_else(error)?, 1.0)
                };
                Ok((finite_number(number, field, path)? * factor).to_bits())
            }
            Difficulty => value.split_ascii_whitespace().try_fold(0, |flags, name| {
                let index = ["EASY", "MEDIUM", "HARD", "BRUTAL"]
                    .iter()
                    .position(|&item| item == name)
                    .ok_or_else(error)?;
                Ok(flags | 1 << index)
            }),
            EnemySorting => [
                "NearestFirst_AllowTies",
                "NearestFirst_BreakTies",
                "Flat",
                "StrongestFirst_AllowTies",
                "StrongestFirst_BreakTies",
            ]
            .iter()
            .position(|&item| item == value)
            .map(|index| index as u32)
            .ok_or_else(error),
            Portrait if value.is_empty() => Ok(0),
            Portrait => Ok(asset_reference(value, PORTRAIT_TYPE, path)?.instance_id),
        }
    }
}

fn put(payload: &mut [u8], offset: usize, value: u32) {
    payload[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn append_string(payload: &mut Vec<u8>, field: usize, value: &str, path: &Path) -> io::Result<()> {
    let bytes = windows_1252(value).ok_or_else(|| {
        invalid(
            path,
            "AI personality name contains characters outside Windows-1252",
        )
    })?;
    let offset = payload.len();
    let end = offset
        .checked_add(bytes.len())
        .and_then(|end| end.checked_add(1))
        .and_then(|end| end.checked_next_multiple_of(4))
        .filter(|&end| end <= i32::MAX as usize)
        .ok_or_else(|| invalid(path, "AI personality name is too large"))?;
    put(payload, field, bytes.len() as u32);
    put(payload, field + 4, offset as u32);
    payload.extend_from_slice(&bytes);
    payload.resize(end, 0);
    Ok(())
}
