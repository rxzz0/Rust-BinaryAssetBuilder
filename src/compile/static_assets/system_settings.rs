use super::ai_strategic::flags;
use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::game_object::encode_inline_object_filter;
use super::global_ui::enum_value;
use super::{
    Marshaled, encode_words, finite_number, percentage, sage_time, sage_uint, significant_children,
    validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

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
