use super::binary::BinaryData;
use super::global_ui::{filter_reference_types, object_filter};
use super::{
    Marshaled, asset_reference, fast_hash, finite_number, percentage, sage_time,
    significant_children, text_value, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
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

    let mut binary = BinaryData {
        payload: vec![0; 512],
        relocations: Vec::new(),
        path,
    };
    for (index, (name, default, kind)) in SCALARS.iter().enumerate() {
        let value = node.attribute(*name).unwrap_or(default).trim();
        binary.word(20 + 4 * index, kind.encode(value, name, path)?);
    }
    for (index, name) in FLAGS.into_iter().enumerate() {
        binary.payload[508 + index] = match node.attribute(name).unwrap_or("false").trim() {
            "true" | "1" => 1,
            "false" | "0" => 0,
            _ => return Err(invalid(path, format!("invalid AI personality {name}"))),
        };
    }
    binary.string(4, required(node, "PersonalityType", path)?)?;
    if let Some(ui_name) = node.attribute("PersonalityUIName") {
        binary.string(12, ui_name)?;
    }

    let mut data = Marshaled::default();
    if !node
        .attribute("CommanderPortrait")
        .unwrap_or(DEFAULT_PORTRAIT)
        .trim()
        .is_empty()
    {
        data.reference_types
            .push(fast_hash(PORTRAIT_TYPE.as_bytes()));
    }
    let mut children: [Vec<_>; CHILDREN.len()] = std::array::from_fn(|_| Vec::new());
    for item in significant_children(node) {
        let name = item.tag_name().name();
        let index = child_index(item, path)?;
        let import = match name {
            "Side" => Some((text_value(item, path)?, "PlayerTemplate")),
            "PowerChoice" => item
                .attribute("PlayerPower")
                .map(|value| (value, "PlayerTech")),
            "StrategicState" => {
                Some((required(item, "State", path)?, "AIStrategicStateDefinition"))
            }
            "BudgetState" => Some((required(item, "State", path)?, "AIBudgetStateDefinition")),
            "BuildState" => Some((required(item, "State", path)?, "AIBuildStateDefinition")),
            _ => None,
        };
        let mut ordinal = 0;
        if let Some((value, target)) = import.filter(|(value, _)| !value.trim().is_empty()) {
            let id = asset_reference(reference_name(value), target, path)?;
            data.reference_types.push(id.type_id);
            data.references.push(id.into());
            ordinal = data.references.len() as u32;
        }
        if name == "SpecificUnitCap"
            && item
                .attribute("Unit")
                .is_some_and(|value| !value.trim().is_empty())
        {
            data.reference_types.push(fast_hash(b"GameObject"));
        }
        if name == FILTER {
            data.reference_types.extend(filter_reference_types(item));
        }
        children[index].push((item, ordinal));
    }

    for ((name, field, size), items) in CHILDREN.into_iter().zip(children) {
        match name {
            CRITERIA => match items.as_slice() {
                [] => (),
                [(item, _)] => write_base_criteria(*item, field, &mut binary)?,
                _ => return Err(invalid(path, "multiple AI base selection criteria")),
            },
            FILTER => {
                let [(item, _)] = items.as_slice() else {
                    return Err(invalid(path, "expected one AI structure filter"));
                };
                object_filter(*item, field, &mut binary)?;
            }
            _ => {
                let first = binary.list(field, items.len(), size)?;
                for (index, (item, ordinal)) in items.into_iter().enumerate() {
                    let offset = first + index * size;
                    write_item(item, name, offset, ordinal, &mut binary, &mut data)?;
                }
            }
        }
    }
    binary.relocations.sort_unstable();
    binary.relocations.push(u32::MAX);
    if !data.imports.is_empty() {
        data.imports.sort_unstable();
        data.imports.push(u32::MAX);
    }
    data.relocations = binary.relocations;
    data.payload = binary.payload;
    Ok(data)
}

pub(super) const CRITERIA: &str = "DefaultTacticHomeBaseCriteria";
pub(super) const FILTER: &str = "CountAsStructureForBuildPurposesFilter";
const ALL_DIFFICULTIES: &str = "EASY MEDIUM HARD BRUTAL";

// Schema order also determines the allocation order of the trailing data.
pub(super) const CHILDREN: [(&str, usize, usize); 11] = [
    ("Side", 236, 4),
    ("UnitCap", 252, 8),
    ("ResourceMultiplierCheat", 300, 8),
    ("StrategicDirectiveFeedback", 308, 12),
    ("PowerChoice", 324, 8),
    ("StrategicState", 340, 12),
    ("BudgetState", 348, 12),
    ("BuildState", 356, 12),
    ("SpecificUnitCap", 364, 12),
    (CRITERIA, 380, 20),
    (FILTER, 384, 112),
];

pub(super) fn child_index(node: Node<'_, '_>, path: &Path) -> io::Result<usize> {
    CHILDREN
        .iter()
        .position(|(tag, _, _)| node.has_tag_name((SAGE_NS, *tag)))
        .ok_or_else(|| {
            invalid(
                path,
                format!(
                    "unsupported AI personality child {}",
                    node.tag_name().name()
                ),
            )
        })
}

pub(super) fn reference_name(value: &str) -> &str {
    value.split_once('\\').map_or(value, |(name, _)| name)
}

fn write_item(
    node: Node<'_, '_>,
    name: &str,
    offset: usize,
    ordinal: u32,
    binary: &mut BinaryData<'_>,
    data: &mut Marshaled,
) -> io::Result<()> {
    let path = binary.path;
    if name != "Side" && has_content(node) {
        return Err(invalid(path, format!("unexpected content in {name}")));
    }
    let value = |field, default, kind: FieldKind| {
        kind.encode(node.attribute(field).unwrap_or(default).trim(), field, path)
    };
    let difficulty = || value("Difficulty", ALL_DIFFICULTIES, Difficulty);
    match name {
        "Side" => {
            binary.word(offset, ordinal);
            data.imports.push(offset as u32);
        }
        "UnitCap" => {
            validate_attributes(node, &["Cap", "Difficulty"], path)?;
            binary.word(offset, value("Cap", "100", Signed)?);
            binary.word(offset + 4, difficulty()?);
        }
        "ResourceMultiplierCheat" => {
            validate_attributes(node, &["Percentage", "Difficulty"], path)?;
            binary.word(offset, value("Percentage", "100%", Percent)?);
            binary.word(offset + 4, difficulty()?);
        }
        "StrategicDirectiveFeedback" => {
            use super::global_ui::enum_value;
            validate_attributes(node, &["StrategicDirective", "Event", "FeedbackKey"], path)?;
            let directive = enum_value(
                required(node, "StrategicDirective", path)?,
                &[
                    "Invalid",
                    "Normal",
                    "Assault",
                    "HoldPosition",
                    "HoldObject",
                    "Help",
                ],
                path,
            )?
            .wrapping_sub(1);
            let event = enum_value(
                required(node, "Event", path)?,
                &["Begin", "Complete", "Cancel"],
                path,
            )?;
            binary.word(offset, directive);
            binary.word(offset + 4, event);
            binary.word(
                offset + 8,
                fast_hash(required(node, "FeedbackKey", path)?.as_bytes()),
            );
        }
        "PowerChoice" => {
            validate_attributes(node, &["PlayerPower", "Weight"], path)?;
            if ordinal != 0 {
                let pointer = binary.allocate(offset, 4)?;
                binary.word(pointer, ordinal);
                data.imports.push(pointer as u32);
            }
            binary.word(offset + 4, value("Weight", "20%", Percent)?);
        }
        "StrategicState" | "BudgetState" | "BuildState" => {
            validate_attributes(node, &["id", "State", "Difficulty"], path)?;
            binary.word(offset + 4, ordinal);
            binary.word(offset + 8, difficulty()?);
            data.imports.push((offset + 4) as u32);
        }
        "SpecificUnitCap" => {
            validate_attributes(node, &["Unit", "Cap", "Difficulty"], path)?;
            if let Some(unit) = node
                .attribute("Unit")
                .filter(|unit| !unit.trim().is_empty())
            {
                binary.word(
                    offset,
                    asset_reference(unit, "GameObject", path)?.instance_id,
                );
            }
            binary.word(offset + 4, value("Cap", "1", Signed)?);
            binary.word(offset + 8, difficulty()?);
        }
        _ => unreachable!("unsupported AI personality record"),
    }
    Ok(())
}

pub(super) fn write_base_criteria(
    node: Node<'_, '_>,
    pointer: usize,
    binary: &mut BinaryData<'_>,
) -> io::Result<()> {
    const FIELDS: [&str; 4] = [
        "RequiredBaseTypes",
        "ExcludedBaseTypes",
        "PreferredBaseTypes",
        "PreferredNotBaseTypes",
    ];
    const FLAGS: [&str; 10] = [
        "MAIN_BASE",
        "EXPANSION",
        "CAPTURED",
        "POINT_CENTERED",
        "LAND_CENTERED",
        "UNDER_ATTACK",
        "ENEMIES_IN_BASE",
        "OLDEST_OF_TYPE",
        "MOST_RECENT_OF_TYPE",
        "NO_BUILD_RADIUS",
    ];
    const SORT: [&str; 7] = [
        "PREFER_OLDEST",
        "PREFER_NEWEST",
        "PREFER_SMALLEST",
        "PREFER_LARGEST",
        "PREFER_MOST_RECENTLY_ATTACKED",
        "PREFER_LEAST_RECENTLY_ATTACKED",
        "PREFER_CLOSEST",
    ];
    let path = binary.path;
    let allowed = [FIELDS.as_slice(), &["SortOrder"]].concat();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected content in AI base selection criteria",
        ));
    }
    let offset = binary.allocate(pointer, 20)?;
    for (index, field) in FIELDS.into_iter().enumerate() {
        let mut flags = 0;
        for flag in node.attribute(field).unwrap_or("").split_ascii_whitespace() {
            let flag = if flag == "PRIMARY_TYPE_MAX" {
                "CAPTURED"
            } else {
                flag
            };
            let bit = FLAGS
                .iter()
                .position(|&value| value == flag)
                .ok_or_else(|| invalid(path, format!("invalid AI base type {flag}")))?;
            flags |= 1 << bit;
        }
        binary.word(offset + 4 * index, flags);
    }
    let sort = node
        .attribute("SortOrder")
        .unwrap_or("PREFER_NEWEST")
        .trim();
    let sort = SORT
        .iter()
        .position(|&value| value == sort)
        .ok_or_else(|| invalid(path, format!("invalid AI base sort order {sort}")))?;
    binary.word(offset + 16, sort as u32);
    Ok(())
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
            Time => Ok(sage_time(value, field, path)?.to_bits()),
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
