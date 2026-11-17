use super::ai_strategic::{boolean_bytes, enum_index, flags};
use super::binary::BinaryData;
use super::global_ui::{filter_reference_types, kind_of_flags, object_filter};
use super::{
    Marshaled, finite_number, sage_angle, sage_time, schema_enums, significant_children,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const SCHEMA: &str =
    include_str!("../../../schemas/ra3/xsd/SkirmishAI/AssetTypeAISpecialPower.xsd");

pub(super) fn write_info(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    const REALS: [(&str, usize, &str); 7] = [
        ("Distance", 8, "100"),
        ("HealthFraction", 12, "0"),
        (
            "InvulnerabilityDamageFractionWhenOutOfRangeOfTarget",
            100,
            ".3",
        ),
        (
            "InvulnerabilityDamageFractionWhenInRangeOfTarget",
            104,
            ".6",
        ),
        ("MaxTargetSpeed", 112, "99999"),
        ("MinKillValue", 116, "0"),
        ("SearchRadius", 140, "150"),
    ];
    const BOOLS: [(&str, bool); 4] = [
        ("NeverUseWhileRetreating", false),
        ("NeverUseWhilePullingBack", false),
        ("RequireKillTargets", true),
        ("UseNearTacticalTarget", false),
    ];
    const LOOK_AHEAD: &str = "InvulnerabilityDamageLookAheadTime";
    const LOOK_BEHIND: &str = "InvulnerabilityDamageLookBehindTime";
    const ANGLE: &str = "InvulnerabilityAvoidUseIfAngleToNextPathNodeIsMoreThan";
    let path = data.path;
    let mut allowed = vec![
        "id",
        "Heuristic",
        "Manager",
        "TargetObjectInclude",
        "TargetObjectExclude",
        "UseOnlyWhenPowerAdvantageIsAbove",
        LOOK_AHEAD,
        LOOK_BEHIND,
        ANGLE,
        "MinTargetsHit",
        "MinEnemiesNearby",
        "NextLogicCommand",
        "UseInMicroManagerState",
        "UseWhenOutOfAmmo",
        "UpdateObject",
        "Location",
        "MaxUpdatables",
        "UseToCounterGarrison",
    ];
    allowed.extend(REALS.iter().map(|&(name, _, _)| name));
    allowed.extend(BOOLS.iter().map(|&(name, _)| name));
    validate_attributes(node, &allowed, path)?;

    static ENUMS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    let enums = ENUMS
        .get_or_init(|| schema_enums(SCHEMA, ["AISpecialPowerHeuristic", "AISpecialPowerManager"]));
    let start = data.allocate(24, 172)?;
    for (index, field) in ["Heuristic", "Manager"].into_iter().enumerate() {
        data.word(
            start + 4 * index,
            enum_index(required(node, field, path)?, &enums[index], field, path)?,
        );
    }
    let value = |field, default| node.attribute(field).unwrap_or(default);
    for (field, offset, default) in REALS {
        data.word(
            start + offset,
            finite_number(value(field, default), field, path)?.to_bits(),
        );
    }
    for (field, offset) in [("TargetObjectInclude", 16), ("TargetObjectExclude", 52)] {
        for (index, word) in kind_of_flags(value(field, ""), path)?
            .into_iter()
            .enumerate()
        {
            data.word(start + offset + index * 4, word);
        }
    }
    if let Some(raw) = node.attribute("UseOnlyWhenPowerAdvantageIsAbove") {
        data.optional_word(
            start + 88,
            finite_number(raw, "UseOnlyWhenPowerAdvantageIsAbove", path)?.to_bits(),
        )?;
    }
    for (field, offset) in [(LOOK_AHEAD, 92), (LOOK_BEHIND, 96)] {
        data.word(
            start + offset,
            sage_time(value(field, "1s"), field, path)?.to_bits(),
        );
    }
    data.word(
        start + 108,
        sage_angle(value(ANGLE, "360d"), ANGLE, path)?.to_bits(),
    );
    for (field, offset) in [("MinTargetsHit", 120), ("MinEnemiesNearby", 124)] {
        data.word(start + offset, integer(value(field, "1"), field, path)?);
    }
    if let Some(raw) = node.attribute("NextLogicCommand") {
        let target = data.allocate(start + 128, 4)?;
        write_import(raw, "LogicCommand", target, data, result)?;
    }
    static STATES: OnceLock<[Vec<String>; 1]> = OnceLock::new();
    let states = STATES.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/SkirmishAI/AssetTypeAIMicroManagerStates.xsd"),
            ["MicroManagerState"],
        )
    });
    data.word(
        start + 132,
        flags(
            value("UseInMicroManagerState", ""),
            &states[0],
            "UseInMicroManagerState",
            path,
        )?,
    );
    if let Some(raw) = node.attribute("UpdateObject") {
        let target = data.allocate(start + 136, 4)?;
        write_import(raw, "GameObject", target, data, result)?;
    }
    data.word(
        start + 144,
        flags(
            value("Location", "Water Land"),
            &["Water", "Land"],
            "Location",
            path,
        )?,
    );
    if let Some(raw) = node.attribute("MaxUpdatables") {
        data.optional_word(start + 148, integer(raw, "MaxUpdatables", path)?)?;
    }
    if node.attribute("UseToCounterGarrison").is_some() {
        data.optional_word(
            start + 152,
            boolean_bytes(node, &[("UseToCounterGarrison", false)], path)?,
        )?;
    }
    data.word(start + 164, boolean_bytes(node, &BOOLS, path)?);
    data.word(
        start + 168,
        boolean_bytes(node, &[("UseWhenOutOfAmmo", false)], path)?,
    );

    let mut children = significant_children(node).peekable();
    if let Some(filter) = children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter"))) {
        let target = data.allocate(start + 156, 112)?;
        object_filter(filter, target, data)?;
        result
            .reference_types
            .extend(filter_reference_types(filter));
    }
    if let Some(weapon) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "SpecialPowerWeapon")))
    {
        let target = data.allocate(start + 160, 28)?;
        write_weapon(weapon, target, data, result)?;
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in AISpecialPowerInfo"));
    }
    Ok(())
}

fn write_weapon(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    const BOOLS: [(&str, bool); 4] = [
        ("IsDisablingAttack", false),
        ("AffectsFriendlies", false),
        ("AffectsSelf", false),
        ("ToggleShortRangeTargetForcesTarget", false),
    ];
    const PREFERENCE: &str = "AoeAttackPowerLocationPreference";
    const BLOCK_TIME: &str = "AoeAttackPowerBlockAreaAfterActivationTime";
    const EMANATION: &str = "AoeAttackEmanationLocation";
    const HYSTERESIS: &str = "ToggleRangeHysteresis";
    let path = data.path;
    let mut allowed = vec![
        "Weapon",
        "TotalValueGreaterThan",
        PREFERENCE,
        BLOCK_TIME,
        EMANATION,
        HYSTERESIS,
    ];
    allowed.extend(BOOLS.iter().map(|&(name, _)| name));
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in SpecialPowerWeapon"));
    }

    let value = |field, default| node.attribute(field).unwrap_or(default);
    write_import(
        required(node, "Weapon", path)?,
        "WeaponTemplate",
        start,
        data,
        result,
    )?;
    data.word(
        start + 4,
        integer(
            value("TotalValueGreaterThan", "0"),
            "TotalValueGreaterThan",
            path,
        )?,
    );
    data.word(
        start + 8,
        enum_index(
            value(PREFERENCE, "PREFER_NEAREST"),
            &[
                "PREFER_NEAREST",
                "PREFER_CENTER_OF_ENEMIES",
                "PREFER_MAXIMUM_DAMAGE",
            ],
            PREFERENCE,
            path,
        )?,
    );
    data.word(
        start + 12,
        sage_time(value(BLOCK_TIME, "0s"), BLOCK_TIME, path)?.to_bits(),
    );
    data.word(
        start + 16,
        enum_index(
            value(EMANATION, "EMANATE_FROM_SELF"),
            &["EMANATE_FROM_SELF", "EMANATE_FROM_TARGET"],
            EMANATION,
            path,
        )?,
    );
    data.word(
        start + 20,
        finite_number(value(HYSTERESIS, "0"), HYSTERESIS, path)?.to_bits(),
    );
    data.word(start + 24, boolean_bytes(node, &BOOLS, path)?);
    Ok(())
}

fn integer(value: &str, field: &str, path: &Path) -> io::Result<u32> {
    value
        .trim()
        .parse::<i32>()
        .map(|v| v as u32)
        .map_err(|_| invalid(path, format!("invalid {field}")))
}
