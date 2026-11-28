use super::ai_heuristics::write_heuristics;
use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::global_ui::{filter_reference_types, kind_of_flags, object_filter};
use super::{
    Marshaled, asset_reference, finite_number, hash_name, required_element, sage_time,
    significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const TACTIC_SIZE: usize = 68;
const TEAM_SIZE: usize = 136;
const TACTICS: [&str; 15] = [
    "SimpleAttack",
    "FlankAttack",
    "DefenseAvoidanceAttack",
    "SimpleSiege",
    "Engineer",
    "SimpleDefense",
    "StaticDefense",
    "ReactiveDefense",
    "SimpleExpansion",
    "ExactExpansion",
    "Hunt",
    "RoamingDefense",
    "GarrisonBuilding",
    "Scripted",
    "NullTactic",
];
const DIFFICULTIES: [&str; 4] = ["EASY", "MEDIUM", "HARD", "BRUTAL"];

pub(super) fn state(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let heuristic = required_element(&mut children, "Heuristic", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected AIStateDefinition child"));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let reference_types = write_heuristics(heuristic, 4, &mut data)?;
    Ok(data.finish(Marshaled {
        reference_types,
        ..Marshaled::default()
    }))
}

pub(super) fn state_name(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", "State", "Name"], path)?;
    let name = node
        .attribute("Name")
        .ok_or_else(|| invalid(path, "expected Name attribute"))?;
    let mut binary = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    let mut data = Marshaled::default();
    write_import(
        &mut binary,
        &mut data,
        4,
        required(node, "State", path)?,
        "AIStrategicStateDefinition",
    )?;
    binary.string(8, name)?;
    Ok(binary.finish(data))
}

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &[
            "id",
            "MaxTargets",
            "MaxRunningCopies",
            "MaxInactiveCopies",
            "PathfindingCheckType",
            "CannotAttackTargetReaction",
            "TeamGatherBeginFeedbackKey",
            "ExecutionBeginFeedbackKey",
            "ExecutionCanceledFeedbackKey",
            "ExecutionCompleteFeedbackKey",
        ],
        path,
    )?;
    let mut binary = BinaryData {
        payload: vec![0; 64],
        relocations: Vec::new(),
        path,
    };
    let mut data = Marshaled::default();
    binary.word(
        12,
        uint(
            node.attribute("MaxTargets").unwrap_or("1"),
            "MaxTargets",
            path,
        )?,
    );
    for (field, offset) in [("MaxRunningCopies", 16), ("MaxInactiveCopies", 20)] {
        if let Some(value) = node.attribute(field) {
            binary.optional_word(offset, uint(value, field, path)?)?;
        }
    }
    for (field, offset, values) in [
        (
            "PathfindingCheckType",
            24,
            &["QuickDoesPathExist", "QuickDoesAttackPathExist", "None"][..],
        ),
        (
            "CannotAttackTargetReaction",
            28,
            &[
                "TreatLikeSuccess",
                "Disband",
                "ReturnHomeAndDisband",
                "Ignore",
            ][..],
        ),
    ] {
        if let Some(value) = node.attribute(field) {
            binary.optional_word(offset, enum_index(value, values, field, path)?)?;
        }
    }
    for (field, offset) in [
        ("TeamGatherBeginFeedbackKey", 32),
        ("ExecutionBeginFeedbackKey", 36),
        ("ExecutionCanceledFeedbackKey", 40),
        ("ExecutionCompleteFeedbackKey", 44),
    ] {
        if let Some(value) = node.attribute(field) {
            let bytes = crate::compile::windows_1252(value)
                .ok_or_else(|| invalid(path, format!("invalid {field}")))?;
            binary.optional_word(offset, crate::hash::asset_string_hash(&bytes))?;
        }
    }
    let mut children = significant_children(node);
    let heuristic = required_element(&mut children, "Heuristic", path)?;
    let mut children = children.peekable();
    let mut targets = Vec::new();
    while let Some(target) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "TargetHeuristic")))
    {
        targets.push(target);
    }
    let tactics: Vec<_> = children.collect();
    let targets_start = binary.list(48, targets.len(), 8)?;
    let tactics_start = binary.list(56, tactics.len(), TACTIC_SIZE)?;
    let root_relocations = binary.relocations.len();
    for (index, target) in targets.into_iter().enumerate() {
        validate_attributes(target, &["TargetHeuristic", "Priority"], path)?;
        if has_content(target) {
            return Err(invalid(path, "unexpected content in AI TargetHeuristic"));
        }
        let offset = targets_start + 8 * index;
        write_import(
            &mut binary,
            &mut data,
            offset,
            required(target, "TargetHeuristic", path)?,
            "AITargetingHeuristic",
        )?;
        binary.word(
            offset + 4,
            int(
                target.attribute("Priority").unwrap_or("10"),
                "Priority",
                path,
            )?,
        );
    }
    for (index, tactic) in tactics.into_iter().enumerate() {
        write_tactic(
            tactic,
            tactics_start + index * TACTIC_SIZE,
            &mut binary,
            &mut data,
        )?;
    }
    let heuristic_relocation = binary.relocations.len();
    let mut types = write_heuristics(heuristic, 4, &mut binary)?;
    types.append(&mut data.reference_types);
    data.reference_types = types;
    // The heuristic payload comes last, but its array pointer is a root field.
    binary.relocations.remove(heuristic_relocation);
    binary.relocations.insert(root_relocations, 8);
    binary.relocations.push(u32::MAX);
    if !data.imports.is_empty() {
        data.imports.push(u32::MAX);
    }
    data.payload = binary.payload;
    data.relocations = binary.relocations;
    Ok(data)
}

fn write_tactic(
    node: Node<'_, '_>,
    offset: usize,
    binary: &mut BinaryData<'_>,
    data: &mut Marshaled,
) -> io::Result<()> {
    let path = binary.path;
    if !node.has_tag_name((SAGE_NS, "Tactic")) {
        return Err(invalid(path, "expected an AI Tactic"));
    }
    validate_attributes(
        node,
        &[
            "id",
            "Tactic",
            "EndBehavior",
            "DisbandAfterRetreatTeamSize",
            "AestheticsManagerPatrolIdleTime",
            "AestheticsManagerMinDistanceFromHarvester",
            "Difficulty",
            "Distance",
            "CombatLeashSlack",
            "TargetDispositionToDisplay",
            "UseTransports",
            "UpdateTime",
            "Targetless",
            "EndIfStateTurnsOff",
            "UseAestheticsManager",
        ],
        path,
    )?;
    let value = |name, default| node.attribute(name).unwrap_or(default);
    let tactics = enum_index(required(node, "Tactic", path)?, &TACTICS, "Tactic", path)?;
    let end = enum_index(
        value("EndBehavior", "RAMPAGE"),
        &[
            "RAMPAGE",
            "DISBAND",
            "RETURN_HOME_AND_DISBAND",
            "PICK_NEW_TARGET",
        ],
        "EndBehavior",
        path,
    )?;
    let disposition = enum_index(
        value("TargetDispositionToDisplay", "Invalid"),
        &["Invalid", "Offensive", "Defensive", "Neutral"],
        "TargetDispositionToDisplay",
        path,
    )?;
    let words = [
        hash_name(required(node, "id", path)?),
        tactics,
        end,
        int(
            value("DisbandAfterRetreatTeamSize", "0"),
            "DisbandAfterRetreatTeamSize",
            path,
        )?,
        sage_time(
            value("AestheticsManagerPatrolIdleTime", "10s"),
            "AestheticsManagerPatrolIdleTime",
            path,
        )?
        .to_bits(),
        finite_number(
            value("AestheticsManagerMinDistanceFromHarvester", "300.0"),
            "AestheticsManagerMinDistanceFromHarvester",
            path,
        )?
        .to_bits(),
        flags(
            value("Difficulty", "EASY MEDIUM HARD BRUTAL"),
            &DIFFICULTIES,
            "Difficulty",
            path,
        )?,
        finite_number(value("Distance", "400.0"), "Distance", path)?.to_bits(),
        finite_number(value("CombatLeashSlack", "1.5"), "CombatLeashSlack", path)?.to_bits(),
        disposition.wrapping_sub(1),
        0,
        sage_time(value("UpdateTime", "1.0s"), "UpdateTime", path)?.to_bits(),
    ];
    for (index, word) in words.into_iter().enumerate() {
        binary.word(offset + 4 * index, word);
    }
    binary.word(
        offset + 64,
        boolean_bytes(
            node,
            &[
                ("Targetless", false),
                ("EndIfStateTurnsOff", false),
                ("UseAestheticsManager", true),
                ("UseTransports", true),
            ],
            path,
        )?,
    );
    let teams: Vec<_> = significant_children(node).collect();
    if teams.len() > 4 {
        return Err(invalid(path, "AI tactic has more than four team templates"));
    }
    let first = binary.list(offset + 56, teams.len(), TEAM_SIZE)?;
    for (index, team) in teams.into_iter().enumerate() {
        write_team(team, first + index * TEAM_SIZE, binary, data)?;
    }
    Ok(())
}

fn write_team(
    node: Node<'_, '_>,
    offset: usize,
    binary: &mut BinaryData<'_>,
    data: &mut Marshaled,
) -> io::Result<()> {
    let path = binary.path;
    if !node.has_tag_name((SAGE_NS, "TeamTemplate")) {
        return Err(invalid(path, "expected AI TeamTemplate"));
    }
    validate_attributes(
        node,
        &[
            "MinUnits",
            "MaxUnits",
            "MinPowerAdvantage",
            "MaxTypesOfUnits",
            "IncludeKindOf",
            "ExcludeKindOf",
            "Priority",
            "ReleaseType",
            "AlwaysRecruit",
            "AllowRecruitUnitsInCombat",
            "RequireCanAttackTarget",
            "MicroManager",
            "AllowedLocomotorTypes",
            "SortByDistance",
            "DistanceSortPoint",
            "RegisteredObjectSet",
            "IsBaseDefenseTeam",
            "BaseDefenseMaxDistance",
        ],
        path,
    )?;
    let value = |name, default| node.attribute(name).unwrap_or(default);
    binary.word(offset, int(value("MinUnits", "1"), "MinUnits", path)?);
    for (field, relative) in [("MaxUnits", 4), ("MaxTypesOfUnits", 12)] {
        if let Some(value) = node.attribute(field) {
            binary.optional_word(offset + relative, int(value, field, path)?)?;
        }
        if relative == 4
            && let Some(value) = node.attribute("MinPowerAdvantage")
        {
            binary.optional_word(
                offset + 8,
                finite_number(value, "MinPowerAdvantage", path)?.to_bits(),
            )?;
        }
    }
    for (field, default, relative) in [
        ("IncludeKindOf", "", 16),
        ("ExcludeKindOf", "HARVESTER IGNORES_SELECT_ALL", 52),
    ] {
        for (index, word) in kind_of_flags(value(field, default), path)?
            .into_iter()
            .enumerate()
        {
            binary.word(offset + relative + 4 * index, word);
        }
    }
    binary.word(
        offset + 88,
        enum_index(
            value("Priority", "STANDARD"),
            &[
                "DEFAULT",
                "DEFENSE",
                "LOW",
                "STANDARD",
                "DEFENSE_HIGH",
                "HIGH",
                "COOP",
                "SCRIPTED",
                "HOTSPOT",
            ],
            "Priority",
            path,
        )?,
    );
    binary.word(
        offset + 92,
        enum_index(
            value("ReleaseType", "RELEASE_TO_PRIORITY"),
            &["ALWAYS_RELEASE", "RELEASE_TO_PRIORITY", "NEVER_RELEASE"],
            "ReleaseType",
            path,
        )?,
    );
    write_import(
        binary,
        data,
        offset + 96,
        required(node, "MicroManager", path)?,
        "AIMicroManagerData",
    )?;
    binary.word(
        offset + 100,
        flags(
            value("AllowedLocomotorTypes", "LAND WATER AMPHIBIOUS AIR"),
            &["LAND", "WATER", "AMPHIBIOUS", "AIR"],
            "AllowedLocomotorTypes",
            path,
        )?,
    );
    binary.word(
        offset + 104,
        enum_index(
            value("DistanceSortPoint", "TARGET_POINT"),
            &["TARGET_POINT", "HOME_BASE"],
            "DistanceSortPoint",
            path,
        )?,
    );
    binary.word(
        offset + 108,
        registered_object_set(value("RegisteredObjectSet", "UNITS"), path)?,
    );
    if let Some(distance) = node.attribute("BaseDefenseMaxDistance") {
        binary.optional_word(
            offset + 112,
            finite_number(distance, "BaseDefenseMaxDistance", path)?.to_bits(),
        )?;
    }
    binary.word(
        offset + 128,
        boolean_bytes(
            node,
            &[
                ("AlwaysRecruit", false),
                ("AllowRecruitUnitsInCombat", false),
                ("RequireCanAttackTarget", false),
                ("SortByDistance", true),
            ],
            path,
        )?,
    );
    binary.word(
        offset + 132,
        boolean_bytes(node, &[("IsBaseDefenseTeam", false)], path)?,
    );

    let mut children = significant_children(node).peekable();
    let mut units = Vec::new();
    while let Some(unit) = children.next_if(|child| child.has_tag_name((SAGE_NS, "CreateUnits"))) {
        units.push(unit);
    }
    if units.len() > 7 {
        return Err(invalid(
            path,
            "AI team template has more than seven create-unit records",
        ));
    }
    let first = binary.list(offset + 116, units.len(), 16)?;
    for (index, unit) in units.into_iter().enumerate() {
        validate_attributes(
            unit,
            &["MinUnits", "MaxUnits", "UnitName", "ExperienceLevel"],
            path,
        )?;
        if has_content(unit) {
            return Err(invalid(path, "unexpected content in AI CreateUnits"));
        }
        let offset = first + 16 * index;
        let id = asset_reference(required(unit, "UnitName", path)?, "GameObject", path)?;
        data.reference_types.push(id.type_id);
        binary.word(offset + 8, id.instance_id);
        for (name, relative) in [("MinUnits", 0), ("MaxUnits", 4), ("ExperienceLevel", 12)] {
            binary.word(
                offset + relative,
                int(unit.attribute(name).unwrap_or("1"), name, path)?,
            );
        }
    }
    if let Some(filter) = children.next() {
        if !filter.has_tag_name((SAGE_NS, "ObjectFilter")) || children.next().is_some() {
            return Err(invalid(path, "unexpected content in AI TeamTemplate"));
        }
        let start = binary.allocate(offset + 124, 112)?;
        object_filter(filter, start, binary)?;
        data.reference_types.extend(filter_reference_types(filter));
    }
    Ok(())
}

fn write_import(
    binary: &mut BinaryData<'_>,
    data: &mut Marshaled,
    offset: usize,
    value: &str,
    kind: &str,
) -> io::Result<()> {
    let id = asset_reference(value, kind, binary.path)?;
    data.reference_types.push(id.type_id);
    data.references.push(id.into());
    data.imports.push(offset as u32);
    binary.word(offset, data.references.len() as u32);
    Ok(())
}

fn uint(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    value
        .trim()
        .parse()
        .map_err(|_| invalid(path, format!("invalid {name}")))
}

fn int(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    value
        .trim()
        .parse::<i32>()
        .map(|number| number as u32)
        .map_err(|_| invalid(path, format!("invalid {name}")))
}

pub(super) fn enum_index(
    value: &str,
    values: &[impl AsRef<str>],
    name: &str,
    path: &Path,
) -> io::Result<u32> {
    values
        .iter()
        .position(|candidate| candidate.as_ref() == value.trim())
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("invalid {name} value {value:?}")))
}

pub(super) fn flags(
    value: &str,
    values: &[impl AsRef<str>],
    name: &str,
    path: &Path,
) -> io::Result<u32> {
    value.split_ascii_whitespace().try_fold(0, |bits, flag| {
        Ok(bits | 1 << enum_index(flag, values, name, path)?)
    })
}

pub(super) fn boolean_bytes(
    node: Node<'_, '_>,
    fields: &[(&str, bool)],
    path: &Path,
) -> io::Result<u32> {
    let mut bits = 0;
    for (index, (name, default)) in fields.iter().enumerate() {
        let value = match node.attribute(*name).map(str::trim) {
            None => *default,
            Some("true" | "1") => true,
            Some("false" | "0") => false,
            _ => return Err(invalid(path, format!("invalid {name}"))),
        };
        bits |= u32::from(value) << (8 * index);
    }
    Ok(bits)
}

fn registered_object_set(value: &str, path: &Path) -> io::Result<u32> {
    static SETS: OnceLock<Vec<String>> = OnceLock::new();
    let sets = SETS.get_or_init(|| {
        let schema = Document::parse(include_str!(
            "../../../schemas/ra3/xsd/Includes/RegisteredObjectSetId.xsd"
        ))
        .expect("bundled RegisteredObjectSetId schema must be valid");
        schema
            .descendants()
            .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "enumeration")))
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    });
    sets.iter()
        .position(|set| set == value.trim())
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("unknown registered object set {value:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategic_feedback_keys_match_ea_optional_hashes() {
        let path = Path::new("Probe.xml");
        let document = Document::parse(
            r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
                <AIStrategicStateDefinition id="Absent"><Heuristic><ConstantHeuristic Weight="1"/></Heuristic></AIStrategicStateDefinition>
                <AIStrategicStateDefinition id="Empty" ExecutionBeginFeedbackKey=""><Heuristic><ConstantHeuristic Weight="1"/></Heuristic></AIStrategicStateDefinition>
                <AIStrategicStateDefinition id="Populated" TeamGatherBeginFeedbackKey="GatherKey" ExecutionBeginFeedbackKey="BeginKey"
                    ExecutionCanceledFeedbackKey="CancelKey" ExecutionCompleteFeedbackKey="CompleteKey"><Heuristic><ConstantHeuristic Weight="1"/></Heuristic></AIStrategicStateDefinition>
            </AssetDeclaration>"#,
        )
        .unwrap();
        let states: Vec<_> = significant_children(document.root_element())
            .map(|node| compile(node, path).unwrap())
            .collect();
        let words = |bytes: &[u8]| {
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .copied()
                .map(u32::from_le_bytes)
                .collect::<Vec<_>>()
        };

        assert_eq!(words(&states[0].payload[32..48]), [0, 0, 0, 0]);
        assert_eq!(words(&states[1].payload[32..48]), [0, 64, 0, 0]);
        assert_eq!(words(&states[1].payload[64..68]), [0]);
        assert_eq!(states[1].relocations, [36, 8, 68, u32::MAX]);
        assert_eq!(words(&states[2].payload[32..48]), [64, 68, 72, 76]);
        assert_eq!(
            words(&states[2].payload[64..80]),
            [0xcb48_c7d3, 0x7d4f_ef91, 0xf9c4_fdcf, 0x8060_9f6d]
        );
        assert_eq!(states[2].relocations, [32, 36, 40, 44, 8, 80, u32::MAX]);
    }
}
