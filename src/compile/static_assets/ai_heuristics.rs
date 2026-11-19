use super::ai_strategic::flags;
use super::binary::BinaryData;
use super::global_ui::{filter_reference_types, kind_of_flags, object_filter};
use super::{
    asset_reference, fast_hash, finite_number, percentage, required_element, sage_int, sage_time,
    sage_uint, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

const SURFACES: [&str; 2] = ["Water", "Land"];
const LOCOMOTOR_CONDITIONS: [&str; 19] = [
    "INVALID",
    "NORMAL",
    "NORMAL_UPGRADED",
    "FREEFALL",
    "WANDER",
    "PANIC",
    "TAXIING",
    "SUPERSONIC",
    "MOUNTED",
    "ENRAGED",
    "SCARED",
    "CONTAINED",
    "COMBO",
    "ATTACKING",
    "MOVING_TO_TRANSPORT",
    "WALL_SCALING",
    "CHANGING_FIRINGARC",
    "BURNINGDEATH",
    "EXITING_PRODUCTION_STRUCTURE",
];

pub(super) fn write_heuristics(
    node: Node<'_, '_>,
    count_field: usize,
    binary: &mut BinaryData<'_>,
) -> io::Result<Vec<u32>> {
    validate_attributes(node, &[], binary.path)?;
    let choices: Vec<_> = significant_children(node).collect();
    if choices.is_empty() {
        return Err(invalid(
            binary.path,
            "expected at least one AI state heuristic",
        ));
    }
    let first = binary.list(count_field, choices.len(), 4)?;
    let list_end = binary.payload.len();
    let relocation_start = binary.relocations.len();
    let mut reference_types = Vec::new();
    for (index, choice) in choices.into_iter().enumerate() {
        write_heuristic(choice, first + 4 * index, binary, &mut reference_types)?;
    }
    // Heuristic pointers are grouped ahead of pointers in the heuristic records.
    binary.relocations[relocation_start..].sort_by_key(|&offset| offset as usize >= list_end);
    Ok(reference_types)
}

fn write_heuristic(
    node: Node<'_, '_>,
    pointer: usize,
    binary: &mut BinaryData<'_>,
    references: &mut Vec<u32>,
) -> io::Result<()> {
    let path = binary.path;
    let name = node.tag_name().name();
    let (size, allowed, filter_name): (usize, &[&str], Option<&str>) =
        match (node.tag_name().namespace(), name) {
            (Some(SAGE_NS), "ConstantHeuristic") => (8, &["Weight"], None),
            (Some(SAGE_NS), "TimerHeuristic") => (16, &["StartTime", "EndTime", "CountFrom"], None),
            (Some(SAGE_NS), "IntervalHeuristic") => {
                (16, &["IntervalTime", "ActiveTime", "CountFrom"], None)
            }
            (Some(SAGE_NS), "PathToTargetHeuristic") => (
                20,
                &[
                    "Surface",
                    "ObjectTypeToUseForTest",
                    "LocomotorConditionToUseForTest",
                    "PathExists",
                ],
                None,
            ),
            (Some(SAGE_NS), "ObjectOfTypeExistsHeuristic") => {
                (124, &["Location", "PassIfExists"], Some("ObjectFilter"))
            }
            (Some(SAGE_NS), "EnemyNearbyHeuristic") => (
                20,
                &["Distance", "MinEnemyValue", "EnemyNearby"],
                Some("EnemyFilter"),
            ),
            (Some(SAGE_NS), "IncomeAdvantageHeuristic") => (12, &["Value", "Above"], None),
            (Some(SAGE_NS), "OpenNodeHeuristic") => (12, &["Exists", "MinDistanceFromEnemy"], None),
            (Some(SAGE_NS), "TimeSinceLastExpansionHeuristic") => (12, &["Time", "Past"], None),
            (Some(SAGE_NS), "PowerAdvantageHeuristic") => (
                84,
                &[
                    "Value",
                    "OwnKindOf",
                    "EnemyKindOf",
                    "Above",
                    "IncludeOwnDefenses",
                    "IncludeEnemyDefenses",
                ],
                None,
            ),
            (Some(SAGE_NS), "OpeningMoveHeuristic") => (8, &["Complete"], None),
            (Some(SAGE_NS), "HarvesterCapHeuristic") => (8, &["MaxHarvesters"], None),
            (Some(SAGE_NS), "ResourceNodeShareHeuristic" | "TechBuildingShareHeuristic") => {
                (12, &["PercentOfFairShare", "Above"], None)
            }
            (Some(SAGE_NS), "MoneyHeuristic") => (16, &["Money", "Account", "Above"], None),
            (Some(SAGE_NS), "IsBuildingHeuristic") => (44, &["KindOf", "PassIfBuilding"], None),
            (
                Some(SAGE_NS),
                "AllNearbyNodesFullHeuristic"
                | "OpponentFundsHeuristic"
                | "ProductionAdvantageHeuristic",
            ) => (4, &[], None),
            (Some(SAGE_NS), "GarrisonHeuristic") => {
                (16, &["NumGarrisons", "Relationship", "Above"], None)
            }
            (Some(SAGE_NS), "StructureOrHarvesterUnderAttackHeuristic") => {
                (12, &["TimeSinceAttacked", "UnderAttack"], None)
            }
            (Some(SAGE_NS), "BridgeExistsHeuristic") => (
                12,
                &[
                    "BridgeHealthMinimumPercentage",
                    "BridgeHealthMaximumPercentage",
                ],
                None,
            ),
            (Some(SAGE_NS), "CanBuildBuildingsOnSurfaceHeuristic") => (
                16,
                &["MinPlacementGridCells", "MaxPlacementGridCells", "Surfaces"],
                None,
            ),
            (Some(SAGE_NS), "ScriptedFlagHeuristic") => (12, &["FlagName"], None),
            (Some(SAGE_NS), "BeaconExistsHeuristic") => (12, &["Type", "Exists"], None),
            (Some(SAGE_NS), "StrategicDirectiveHeuristic") => (12, &["Directive", "On"], None),
            (Some(SAGE_NS), "RedAlertHeuristic") => (8, &["On"], None),
            (Some(SAGE_NS), "LinearCombinationHeuristic") => {
                return write_linear_combination(node, pointer, binary, references);
            }
            _ => {
                return Err(invalid(
                    path,
                    format!("unsupported AI state heuristic {name}"),
                ));
            }
        };
    validate_attributes(node, allowed, path)?;
    let mut children = significant_children(node);
    let filter = children.next();
    if filter
        .is_some_and(|filter| !filter_name.is_some_and(|tag| filter.has_tag_name((SAGE_NS, tag))))
        || children.next().is_some()
    {
        return Err(invalid(path, format!("unexpected content in {name}")));
    }

    let offset = binary.allocate(pointer, size)?;
    let type_name = match name {
        "BeaconExistsHeuristic" => "AIAlliedBeaconExistsHeuristic".to_owned(),
        "StrategicDirectiveHeuristic" | "RedAlertHeuristic" => format!("AI{name}"),
        _ => format!("AIState{name}"),
    };
    binary.word(offset, fast_hash(type_name.as_bytes()));
    let value = |field, default| node.attribute(field).unwrap_or(default).trim();
    let float =
        |field, default| finite_number(value(field, default), field, path).map(f32::to_bits);
    let time = |field, default| sage_time(value(field, default), field, path).map(f32::to_bits);
    let percent = |field, default| percentage(value(field, default), field, path).map(f32::to_bits);
    let uint = |field, default| sage_uint(value(field, default), field, path);
    let flag = |field, default| match node.attribute(field).map(str::trim) {
        None => Ok(u32::from(default)),
        Some("true" | "1") => Ok(1),
        Some("false" | "0") => Ok(0),
        _ => Err(invalid(path, format!("invalid {name} {field}"))),
    };
    match name {
        "ConstantHeuristic" => binary.word(offset + 4, float("Weight", "1.0")?),
        "TimerHeuristic" | "IntervalHeuristic" => {
            let (first, second) = if name == "TimerHeuristic" {
                (time("StartTime", "0s")?, time("EndTime", "-1s")?)
            } else {
                (time("IntervalTime", "60s")?, time("ActiveTime", "30s")?)
            };
            binary.word(offset + 4, first);
            binary.word(offset + 8, second);
            binary.word(
                offset + 12,
                enum_value(
                    value("CountFrom", "MAP_START"),
                    &["MAP_START", "AI_START"],
                    path,
                )?,
            );
        }
        "PathToTargetHeuristic" => {
            let object = asset_reference(
                required(node, "ObjectTypeToUseForTest", path)?,
                "GameObject",
                path,
            )?;
            references.push(object.type_id);
            binary.word(
                offset + 4,
                enum_value(value("Surface", "Land"), &SURFACES, path)?,
            );
            binary.word(offset + 8, object.instance_id);
            let condition = enum_value(
                value("LocomotorConditionToUseForTest", "NORMAL"),
                &LOCOMOTOR_CONDITIONS,
                path,
            )?;
            binary.word(offset + 12, condition.wrapping_sub(1));
            binary.word(offset + 16, flag("PathExists", true)?);
        }
        "ObjectOfTypeExistsHeuristic" => {
            let mut surfaces = 0;
            for surface in value("Location", "Water Land").split_ascii_whitespace() {
                surfaces |= 1 << enum_value(surface, &SURFACES, path)?;
            }
            binary.word(offset + 4, surfaces);
            binary.word(offset + 120, flag("PassIfExists", true)?);
            let filter = filter
                .ok_or_else(|| invalid(path, "missing ObjectOfTypeExistsHeuristic filter"))?;
            object_filter(filter, offset + 8, binary)?;
            references.extend(filter_reference_types(filter));
        }
        "EnemyNearbyHeuristic" => {
            let min_value = value("MinEnemyValue", "0")
                .parse::<u32>()
                .map_err(|_| invalid(path, "invalid EnemyNearbyHeuristic MinEnemyValue"))?;
            binary.word(offset + 4, float("Distance", "1000.0")?);
            binary.word(offset + 8, min_value);
            binary.word(offset + 16, flag("EnemyNearby", true)?);
            if let Some(filter) = filter {
                let start = binary.allocate(offset + 12, 112)?;
                object_filter(filter, start, binary)?;
                references.extend(filter_reference_types(filter));
            }
        }
        "IncomeAdvantageHeuristic" => {
            binary.word(offset + 4, float("Value", "1.0")?);
            binary.word(offset + 8, flag("Above", true)?);
        }
        "OpenNodeHeuristic" => {
            binary.word(offset + 4, float("MinDistanceFromEnemy", "400.0")?);
            binary.word(offset + 8, flag("Exists", true)?);
        }
        "TimeSinceLastExpansionHeuristic" => {
            binary.word(offset + 4, time("Time", "60s")?);
            binary.word(offset + 8, flag("Past", true)?);
        }
        "PowerAdvantageHeuristic" => {
            binary.word(offset + 4, float("Value", "1.0")?);
            for (field, start) in [("OwnKindOf", 8), ("EnemyKindOf", 44)] {
                for (index, word) in kind_of_flags(value(field, "CAN_ATTACK"), path)?
                    .into_iter()
                    .enumerate()
                {
                    binary.word(offset + start + 4 * index, word);
                }
            }
            binary.word(
                offset + 80,
                flag("Above", true)?
                    | flag("IncludeOwnDefenses", true)? << 8
                    | flag("IncludeEnemyDefenses", true)? << 16,
            );
        }
        "OpeningMoveHeuristic" => binary.word(offset + 4, flag("Complete", true)?),
        "HarvesterCapHeuristic" => binary.word(offset + 4, uint("MaxHarvesters", "5")?),
        "ResourceNodeShareHeuristic" | "TechBuildingShareHeuristic" => {
            binary.word(offset + 4, percent("PercentOfFairShare", "100%")?);
            binary.word(offset + 8, flag("Above", false)?);
        }
        "MoneyHeuristic" => {
            binary.word(offset + 4, uint("Money", "1000")?);
            binary.word(offset + 8, uint("Account", "0")?);
            binary.word(offset + 12, flag("Above", true)?);
        }
        "IsBuildingHeuristic" => {
            for (index, word) in kind_of_flags(value("KindOf", ""), path)?
                .into_iter()
                .enumerate()
            {
                binary.word(offset + 4 + 4 * index, word);
            }
            binary.word(offset + 40, flag("PassIfBuilding", true)?);
        }
        "AllNearbyNodesFullHeuristic"
        | "OpponentFundsHeuristic"
        | "ProductionAdvantageHeuristic" => {}
        "GarrisonHeuristic" => {
            binary.word(offset + 4, uint("NumGarrisons", "0")?);
            binary.word(
                offset + 8,
                enum_value(
                    value("Relationship", "SAME_PLAYER"),
                    &["ALLIES", "ENEMIES", "NEUTRAL", "SAME_PLAYER"],
                    path,
                )?,
            );
            binary.word(offset + 12, flag("Above", true)?);
        }
        "StructureOrHarvesterUnderAttackHeuristic" => {
            binary.word(offset + 4, time("TimeSinceAttacked", "4s")?);
            binary.word(offset + 8, flag("UnderAttack", true)?);
        }
        "BridgeExistsHeuristic" => {
            binary.word(offset + 4, percent("BridgeHealthMinimumPercentage", "50%")?);
            binary.word(
                offset + 8,
                percent("BridgeHealthMaximumPercentage", "100%")?,
            );
        }
        "CanBuildBuildingsOnSurfaceHeuristic" => {
            binary.word(
                offset + 4,
                sage_int(
                    value("MinPlacementGridCells", "1"),
                    "MinPlacementGridCells",
                    path,
                )?,
            );
            if let Some(maximum) = node.attribute("MaxPlacementGridCells") {
                binary.optional_word(
                    offset + 8,
                    sage_int(maximum, "MaxPlacementGridCells", path)?,
                )?;
            }
            let surfaces = node
                .attribute("Surfaces")
                .ok_or_else(|| invalid(path, "expected Surfaces attribute"))?;
            binary.word(
                offset + 12,
                flags(surfaces, &["LAND", "WATER", "DEEP_WATER"], "Surfaces", path)?,
            );
        }
        "ScriptedFlagHeuristic" => {
            if let Some(name) = node.attribute("FlagName") {
                binary.string(offset + 4, name)?;
            }
        }
        "BeaconExistsHeuristic" => {
            binary.word(
                offset + 4,
                enum_value(
                    value("Type", "Assault"),
                    &["Invalid", "Assault", "HoldPosition", "HoldObject"],
                    path,
                )?
                .wrapping_sub(1),
            );
            binary.word(offset + 8, flag("Exists", false)?);
        }
        "StrategicDirectiveHeuristic" => {
            binary.word(
                offset + 4,
                enum_value(
                    value("Directive", "Normal"),
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
                .wrapping_sub(1),
            );
            binary.word(offset + 8, flag("On", false)?);
        }
        "RedAlertHeuristic" => binary.word(offset + 4, flag("On", true)?),
        _ => unreachable!("validated AI state heuristic"),
    }
    Ok(())
}

fn write_linear_combination(
    node: Node<'_, '_>,
    pointer: usize,
    binary: &mut BinaryData<'_>,
    references: &mut Vec<u32>,
) -> io::Result<()> {
    let path = binary.path;
    validate_attributes(node, &[], path)?;
    let weights: Vec<_> = significant_children(node).collect();
    if weights.len() < 2 {
        return Err(invalid(
            path,
            "LinearCombinationHeuristic requires at least two WeightedHeuristic elements",
        ));
    }
    let offset = binary.allocate(pointer, 12)?;
    binary.word(offset, fast_hash(b"AIStateLinearCombinationHeuristic"));
    let first = binary.list(offset + 4, weights.len(), 8)?;
    for (index, weighted) in weights.into_iter().enumerate() {
        if !weighted.has_tag_name((SAGE_NS, "WeightedHeuristic")) {
            return Err(invalid(path, "expected WeightedHeuristic"));
        }
        validate_attributes(weighted, &["Weight"], path)?;
        let mut children = significant_children(weighted);
        let heuristic = required_element(&mut children, "Heuristic", path)?;
        validate_attributes(heuristic, &[], path)?;
        let mut choices = significant_children(heuristic);
        let choice = choices.next().filter(|choice| {
            matches!(
                choice.tag_name().name(),
                "TimerHeuristic"
                    | "ConstantHeuristic"
                    | "PowerAdvantageHeuristic"
                    | "OpponentFundsHeuristic"
                    | "ProductionAdvantageHeuristic"
                    | "BridgeExistsHeuristic"
            )
        });
        let choice =
            choice.ok_or_else(|| invalid(path, "expected a weighted AI state heuristic"))?;
        if children.next().is_some() || choices.next().is_some() {
            return Err(invalid(path, "unexpected content in WeightedHeuristic"));
        }
        let entry = first + index * 8;
        let weight = finite_number(
            weighted.attribute("Weight").unwrap_or("0.5").trim(),
            "Weight",
            path,
        )?;
        binary.word(entry, weight.to_bits());
        write_heuristic(choice, entry + 4, binary, references)?;
    }
    Ok(())
}

fn enum_value(value: &str, options: &[&str], path: &Path) -> io::Result<u32> {
    options
        .iter()
        .position(|&option| option == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("unknown AI state heuristic value {value:?}")))
}
