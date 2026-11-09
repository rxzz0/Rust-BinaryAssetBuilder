use super::ai_personality::write_base_criteria;
use super::ai_strategic::{boolean_bytes, enum_index, flags};
use super::binary::BinaryData;
use super::global_ui::kind_of_flags;
use super::{Marshaled, finite_number, sage_time, significant_children, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const TARGETS: [&str; 21] = [
    "EnemyStructure",
    "FriendlyStructure",
    "EnemyUnit",
    "FriendlyUnit",
    "UnderAttackBy",
    "EnemyUnitCloseToShore",
    "EnemyStructureCloseToShore",
    "NeutralGarrison",
    "EnemyGarrison",
    "Expansion",
    "TechBuilding",
    "Bridge",
    "Beacon",
    "Crate",
    "Help",
    "Husk",
    "Point",
    "UnpackBuildingPlacement",
    "NamedObject",
    "Waypoint",
    "Team",
];
const SORT: [&str; 6] = [
    "Distance",
    "ThreatToGround",
    "ThreatToAir",
    "ThreatToWater",
    "Random",
    "ClosestToEnemy",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &[
            "id",
            "HeuristicType",
            "SortType",
            "VitalKindOf",
            "ForbiddenKindOf",
            "VitalObjectStatus",
            "ForbiddenObjectStatus",
            "Destroyed",
            "HighestPriorityEnemyPlayersOnly",
            "ThreatFinderRange",
            "SearchRange",
            "Beacon",
            "Name",
            "Location",
            "MaxDistanceToShore",
            "Relationship",
            "UnderAttack",
            "RequireUniqueTarget",
            "MinDistanceFromEnemy",
            "MinDistanceFromConyardEntrance",
            "IgnoreMinDistanceFromConyardEntranceForTime",
        ],
        path,
    )?;
    let mut binary = BinaryData {
        payload: vec![0; 196],
        relocations: Vec::new(),
        path,
    };
    let value = |field, default| node.attribute(field).unwrap_or(default);
    binary.word(
        4,
        enum_index(
            required(node, "HeuristicType", path)?,
            &TARGETS,
            "HeuristicType",
            path,
        )?,
    );
    binary.word(
        8,
        enum_index(value("SortType", "Distance"), &SORT, "SortType", path)?,
    );
    for (field, offset) in [("VitalKindOf", 12), ("ForbiddenKindOf", 48)] {
        for (index, word) in kind_of_flags(value(field, ""), path)?
            .into_iter()
            .enumerate()
        {
            binary.word(offset + 4 * index, word);
        }
    }
    for (field, offset) in [("VitalObjectStatus", 84), ("ForbiddenObjectStatus", 112)] {
        for (index, word) in status_flags(value(field, ""), path)?
            .into_iter()
            .enumerate()
        {
            binary.word(offset + 4 * index, word);
        }
    }
    if let Some(range) = node.attribute("ThreatFinderRange") {
        binary.optional_word(
            140,
            finite_number(range, "ThreatFinderRange", path)?.to_bits(),
        )?;
    }
    binary.word(
        144,
        finite_number(value("SearchRange", "1500.0"), "SearchRange", path)?.to_bits(),
    );
    if let Some(beacon) = node.attribute("Beacon") {
        let beacon = enum_index(
            beacon,
            &["Invalid", "Assault", "HoldPosition", "HoldObject"],
            "Beacon",
            path,
        )?;
        binary.optional_word(148, beacon.wrapping_sub(1))?;
    }
    let name_relocation = if let Some(name) = node.attribute("Name") {
        let start = binary.allocate(152, 8)?;
        binary.string(start, name)?;
        binary.relocations.pop()
    } else {
        None
    };
    binary.word(
        156,
        flags(
            value("Location", "Water Land"),
            &["Water", "Land"],
            "Location",
            path,
        )?,
    );
    binary.word(
        160,
        finite_number(
            value("MaxDistanceToShore", "1000.0"),
            "MaxDistanceToShore",
            path,
        )?
        .to_bits(),
    );
    if let Some(relationship) = node.attribute("Relationship") {
        binary.optional_word(
            164,
            flags(
                relationship,
                &["ALLIES", "ENEMIES", "NEUTRAL", "SAME_PLAYER"],
                "Relationship",
                path,
            )?,
        )?;
    }
    for (field, offset) in [
        ("MinDistanceFromEnemy", 168),
        ("MinDistanceFromConyardEntrance", 172),
    ] {
        binary.word(
            offset,
            finite_number(value(field, "0.0"), field, path)?.to_bits(),
        );
    }
    let field = "IgnoreMinDistanceFromConyardEntranceForTime";
    binary.word(176, sage_time(value(field, "0s"), field, path)?.to_bits());
    binary.word(
        192,
        boolean_bytes(
            node,
            &[
                ("Destroyed", false),
                ("HighestPriorityEnemyPlayersOnly", true),
                ("UnderAttack", false),
                ("RequireUniqueTarget", false),
            ],
            path,
        )?,
    );

    let mut children = significant_children(node).peekable();
    if let Some(criteria) = children.next_if(|child| child.has_tag_name((SAGE_NS, "BaseSelection")))
    {
        write_base_criteria(criteria, 180, &mut binary)?;
    }
    let mut distances = Vec::new();
    for distance in children {
        if !distance.has_tag_name((SAGE_NS, "BuildingPlacementDistance")) || has_content(distance) {
            return Err(invalid(
                path,
                "unexpected content in AI targeting heuristic",
            ));
        }
        validate_attributes(distance, &["Distance"], path)?;
        distances.push(finite_number(
            required(distance, "Distance", path)?,
            "Distance",
            path,
        )?);
    }
    distances.sort_by(f32::total_cmp);
    let first = binary.list(184, distances.len(), 4)?;
    for (index, distance) in distances.into_iter().enumerate() {
        binary.word(first + 4 * index, distance.to_bits());
    }
    binary.relocations.extend(name_relocation);
    if !binary.relocations.is_empty() {
        binary.relocations.push(u32::MAX);
    }
    Ok(Marshaled {
        payload: binary.payload,
        relocations: binary.relocations,
        ..Marshaled::default()
    })
}

fn status_flags(value: &str, path: &Path) -> io::Result<[u32; 7]> {
    static STATUS: OnceLock<Vec<String>> = OnceLock::new();
    let mut words = [0; 7];
    if value.trim().is_empty() {
        return Ok(words);
    }
    let status = STATUS.get_or_init(|| {
        let schema = Document::parse(include_str!(
            "../../../schemas/ra3/xsd/Includes/ObjectStatus.xsd"
        ))
        .expect("bundled ObjectStatus schema must be valid");
        schema
            .descendants()
            .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "enumeration")))
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    });
    for flag in value.split_ascii_whitespace() {
        let bit = status
            .iter()
            .position(|entry| entry == flag)
            .filter(|&bit| bit < 32 * words.len())
            .ok_or_else(|| invalid(path, format!("unknown ObjectStatus flag {flag:?}")))?;
        words[bit / 32] |= 1 << (bit % 32);
    }
    Ok(words)
}
