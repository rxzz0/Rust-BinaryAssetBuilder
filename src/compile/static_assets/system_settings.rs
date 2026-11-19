use super::ai_strategic::flags;
use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::game_object::{encode_inline_object_filter, write_model_conditions};
use super::global_ui::enum_value;
use super::{
    Marshaled, encode_words, finite_number, percentage, required_element, sage_angle, sage_int,
    sage_time, sage_uint, significant_children, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn unit_ai(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const INTS: &[(&str, &str, usize)] = &[
        ("SupplyBoxesPerTree", "90", 4),
        ("ValuePerSupplyBox", "400", 8),
        ("MaxLineBuildObjects", "50", 20),
        ("MaxTunnelCapacity", "3", 24),
        ("TimeAfterDamageUntilRepairAllowed", "0", 56),
        ("InvisibilityOpacityCycleFrames", "60", 76),
        ("DefaultStartingCash", "1500", 100),
    ];
    const REALS: &[(&str, &str, usize)] = &[
        ("MinDistFromEdgeOfMapForBuild", "30.0", 12),
        ("SupplyBuildBorder", "20.0", 16),
        ("MinLowEnergyProductionSpeed", "0.5", 28),
        ("MaxLowEnergyProductionSpeed", "0.8", 32),
        ("LowEnergyPenaltyModifier", "1.0", 36),
        ("DamageRadiusMinimumForSplash", "0.0", 40),
        ("DefaultStructureRubbleHeight", "8.0", 52),
        ("SelectionFlashSaturationFactor", "0.5", 60),
        ("GarrisonedRangeMultiplier", "1.5", 64),
        ("GroupMoveClickToGatherAreaFactor", "0.5", 68),
        ("FormationMoveClusterRange", "200.0", 72),
        ("BuilderMoveFromNewStructureDistance", "10.0", 88),
        ("PartitionCellSize", "40.0", 92),
        ("TerrainResourceCellSize", "20.0", 96),
        ("ParachuteDriftSpeed", "2.0", 108),
        ("LinearDragSubmerged", "0.85", 116),
        ("AutomaticReverseDistance", "40", 124),
        ("SkipMoveMinDistance", "10.0", 128),
        ("ScatterDistance", "100.0", 136),
        ("AIIgnoreDuplicateMoveCommandDistance", "20.0", 140),
        ("HumanIgnoreDuplicateMoveCommandDistance", "5.0", 144),
        ("ConsiderHumanDuplicateMoveCommandDistance", "100.0", 148),
        ("CaptureWaitDistance", "20.0", 152),
    ];
    const TIMES: &[(&str, &str, usize)] = &[
        ("UnlookPersistDuration", "10s", 48),
        ("BuilderFadeOutTime", "1s", 80),
        ("BuilderFadeInTime", "1s", 84),
        ("ParachuteLandTime", "1.5s", 112),
        ("CaptureWaitTime", "2s", 156),
    ];
    const SELL: &str = "SellPercentage";
    const DAMAGE: &str = "MovementPenaltyDamageState";
    const REVERSE: &str = "AutomaticReverseAngle";
    const TARGETS: &str = "CompleteTargetingCompareList";
    const CONDITIONS: &str = "CaptureWaitModelConditions";
    const FLASH_COLOR: &str = "SelectionFlashHouseColor";
    const HOUSE_COLOR: &str = "EnableHouseColor";
    let names: Vec<_> = [
        "id",
        SELL,
        DAMAGE,
        REVERSE,
        TARGETS,
        CONDITIONS,
        FLASH_COLOR,
        HOUSE_COLOR,
    ]
    .into_iter()
    .chain(INTS.iter().chain(REALS).chain(TIMES).map(|field| field.0))
    .collect();
    validate_attributes(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 448],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for &(name, default, offset) in INTS {
        data.word(
            offset,
            sage_int(node.attribute(name).unwrap_or(default), name, path)?,
        );
    }
    for &(name, default, offset) in REALS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    for &(name, default, offset) in TIMES {
        data.word(
            offset,
            sage_time(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    data.word(
        44,
        percentage(node.attribute(SELL).unwrap_or("50%"), SELL, path)?.to_bits(),
    );
    data.word(
        104,
        enum_value(
            node.attribute(DAMAGE).unwrap_or("REALLY_DAMAGED"),
            &["PRISTINE", "DAMAGED", "REALLY_DAMAGED", "RUBBLE"],
            path,
        )?,
    );
    data.word(
        120,
        sage_angle(node.attribute(REVERSE).unwrap_or("10d"), REVERSE, path)?.to_bits(),
    );
    if let Some(targets) = node.attribute(TARGETS) {
        write_import(targets, "TargetingCompareList", 132, &mut data, &mut result)?;
    }
    write_model_conditions(node.attribute(CONDITIONS).unwrap_or(""), 160, &mut data)?;
    let mut children = significant_children(node);
    for (name, offset) in [
        ("VictoryConditionUnitObjectFilter", 220),
        ("VictoryConditionStructureObjectFilter", 332),
    ] {
        let filter = required_element(&mut children, name, path)?;
        encode_inline_object_filter(filter, offset, &mut data, &mut result.reference_types)?;
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected UnitAISettings child"));
    }
    data.payload[444] = boolean(node, FLASH_COLOR, false, path)?;
    data.payload[445] = boolean(node, HOUSE_COLOR, true, path)?;
    Ok(data.finish(result))
}

pub(super) fn build(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const VALUES: &[(&str, &str, usize)] = &[
        ("AllowedHeightVariationForBuildingPlacement", "3.0", 4),
        ("RotationalSnapIterations", "4.0", 8),
        ("BuildabilityDisplayRadius", "90.0", 12),
        ("BuildabilityDecalMargin", "0.142857", 20),
        ("BuildabilityDecalSize", "30.0", 24),
    ];
    const STATIC: &str = "BuildabilityUsesStaticDecalSize";
    const RADIUS: &str = "BuildabilityDecalRadiusPercent";
    const SHAPE: &str = "BuildabilityDisplayShape";
    let names: Vec<_> = ["id", STATIC, RADIUS, SHAPE]
        .into_iter()
        .chain(VALUES.iter().map(|field| field.0))
        .collect();
    validate_leaf(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 36],
        relocations: Vec::new(),
        path,
    };
    for &(name, default, offset) in VALUES {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    let shape = enum_value(
        node.attribute(SHAPE).unwrap_or("CYLINDER"),
        &["SPHERE", "BOX", "CYLINDER", "RING"],
        path,
    )?;
    data.word(16, shape);
    data.word(
        28,
        percentage(node.attribute(RADIUS).unwrap_or("18.5185"), RADIUS, path)?.to_bits(),
    );
    data.payload[32] = boolean(node, STATIC, true, path)?;
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn pathfind(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const NAMES: &[&str] = &[
        "id",
        "MaxPathfindCellsPerFrame",
        "MaxPathfindCellsPerPhase",
        "MaxCellsFindMeleeEngagementLocation",
        "MaxCellsAdjustDestination",
        "MaxCellsAdjustHordeMeleeDestination",
        "MaxCellsAdjustTargetDestination",
        "MaxCellsAdjustToPossibleDestination",
        "MaxCellsAdjustToMeleeDestination",
        "MaxCellsAdjustToNearestGroundCell",
        "MaxCellsAdjustToNearestValidCell",
        "MaxCellsPatchPath",
        "MaxCellsToExamineTowardsGoal",
        "MaxExtraCollisionResponsePathsPerFrame",
        "MaxExtraMoveAwayFromPathPathsPerFrame",
        "LayerZCloseEnough",
    ];
    validate_leaf(node, NAMES, path)?;
    let mut words = vec![0];
    for &name in &NAMES[1..] {
        words.push(sage_uint(required(node, name, path)?, name, path)?);
    }
    Ok(Marshaled {
        payload: encode_words(words),
        ..Marshaled::default()
    })
}

pub(super) fn resource(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const NAME: &str = "SharedMoneyTransactionCategories";
    validate_leaf(node, &["id", NAME], path)?;
    let mask = flags(
        node.attribute(NAME).unwrap_or(""),
        super::money_transaction_categories(),
        NAME,
        path,
    )?;
    Ok(Marshaled {
        payload: encode_words([0, mask]),
        ..Marshaled::default()
    })
}

pub(super) fn invisibility(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let filter = single_child(node, "CamouflageDetectorObjectFilter", path)?;
    let mut data = BinaryData {
        payload: vec![0; 116],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    encode_inline_object_filter(filter, 4, &mut data, &mut result.reference_types)?;
    Ok(data.finish(result))
}

pub(super) fn gift(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let settings = single_child(node, "ObjectGiftSettings", path)?;
    const SKIRMISH: &str = "AllowTransferToSkirmishPlayer";
    validate_attributes(settings, &[SKIRMISH], path)?;
    let filter = single_child(settings, "AllowObjectFilter", path)?;
    let mut data = BinaryData {
        payload: vec![0; 120],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    encode_inline_object_filter(filter, 4, &mut data, &mut result.reference_types)?;
    data.payload[116] = boolean(settings, SKIRMISH, true, path)?;
    Ok(data.finish(result))
}

pub(super) fn build_list_monitor(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const INTERVAL: &str = "TimeBetweenUpdates";
    const COUNTS: &[(&str, &str)] = &[
        ("UpdatesToSkipAfterStart", "4"),
        ("UpdatesToSkipAfterSaveFileLoad", "2"),
        ("UpdatesToSkipAfterTargetPlayerChanges", "2"),
    ];
    let names: Vec<_> = ["id", INTERVAL]
        .into_iter()
        .chain(COUNTS.iter().map(|field| field.0))
        .collect();
    validate_attributes(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 24],
        relocations: Vec::new(),
        path,
    };
    data.word(
        4,
        sage_time(node.attribute(INTERVAL).unwrap_or("0.25s"), INTERVAL, path)?.to_bits(),
    );
    for (index, &(name, default)) in COUNTS.iter().enumerate() {
        data.word(
            8 + index * 4,
            sage_uint(node.attribute(name).unwrap_or(default), name, path)?,
        );
    }
    let mut result = Marshaled::default();
    if significant_children(node).next().is_some() {
        let filter = single_child(node, "ProducerObjectFilter", path)?;
        let start = data.allocate(20, 112)?;
        encode_inline_object_filter(filter, start, &mut data, &mut result.reference_types)?;
    }
    Ok(data.finish(result))
}

fn single_child<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    path: &Path,
) -> io::Result<Node<'a, 'input>> {
    let mut children = significant_children(node);
    children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, name)))
        .filter(|_| children.next().is_none())
        .ok_or_else(|| invalid(path, format!("expected exactly one {name}")))
}
