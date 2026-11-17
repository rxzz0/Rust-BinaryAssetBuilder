use super::ai_strategic::{boolean_bytes, enum_index};
use super::binary::BinaryData;
use super::global_ui::{filter_reference_types, object_filter};
use super::{
    Marshaled, asset_reference, color, finite_number, percentage, required_element,
    significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

const STRINGS: [(&str, usize); 2] = [("DisplayNameToken", 8), ("FactionIconImageName", 16)];
const REFERENCES: [(&str, &str, usize); 10] = [
    ("LoadScreenMusic", "BaseAudioEventInfo", 24),
    ("SkirmishMPVictoryMusic", "BaseAudioEventInfo", 28),
    ("SkirmishMPDefeatMusic", "BaseAudioEventInfo", 32),
    ("FactionDataPathMusicEvent", "PathMusicEvent", 36),
    ("MatchVictoryPathMusicEvent", "PathMusicEvent", 40),
    ("MatchDefeatPathMusicEvent", "PathMusicEvent", 44),
    ("ScriptedHuntTactic", "AIStrategicStateDefinition", 88),
    ("ParachuteOCL", "ObjectCreationList", 92),
    ("DefaultTech", "PlayerTech", 96),
    ("PlayerTechUpgradeBinding", "PlayerTechUpgradeBinding", 100),
];
const WEAK_REFERENCES: [(&str, usize); 5] = [
    ("StartingBuilding", 52),
    ("SpellBookSP", 56),
    ("SpellBookMP", 60),
    ("UnattachedMpBeaconTemplate", 64),
    ("AttachedMpBeaconTemplate", 68),
];
const INTEGERS: [(&str, usize, &str); 4] = [
    ("MaxMpBeacons", 72, "0"),
    ("NumberOfResourceAccounts", 84, "1"),
    ("TechRankPointsPerSecond", 104, "0"),
    ("InitialTechPoints", 108, "0"),
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let allowed: Vec<_> = [
        "id",
        "Type",
        "Side",
        "ResourceModifierValueList",
        "PowerOutageTurnsOffRadar",
    ]
    .into_iter()
    .chain(STRINGS.map(|(name, _)| name))
    .chain(REFERENCES.map(|(name, _, _)| name))
    .chain(WEAK_REFERENCES.map(|(name, _)| name))
    .chain(INTEGERS.map(|(name, _, _)| name))
    .collect();
    validate_attributes(node, &allowed, path)?;
    let mut data = BinaryData {
        payload: vec![0; 188],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let value = |field, default| node.attribute(field).unwrap_or(default);
    data.word(
        4,
        enum_index(
            value("Type", "NONPLAYABLE"),
            &[
                "NONPLAYABLE",
                "PLAYABLE",
                "OBSERVER",
                "RANDOM",
                "COMMENTATOR",
                "NULL",
            ],
            "Type",
            path,
        )?,
    );
    data.word(
        48,
        enum_index(
            value("Side", "Neutral"),
            &["RANDOM", "Allies", "Soviet", "Japan", "Neutral"],
            "Side",
            path,
        )?
        .wrapping_sub(1),
    );
    for (field, offset) in STRINGS {
        if let Some(raw) = node.attribute(field) {
            data.string(offset, raw)?;
        }
    }
    for (field, offset, fallback) in INTEGERS {
        data.word(offset, unsigned(value(field, fallback), field, path)?);
    }
    data.word(
        184,
        boolean_bytes(node, &[("PowerOutageTurnsOffRadar", true)], path)?,
    );
    let modifiers = value("ResourceModifierValueList", "").split_ascii_whitespace();
    let start = data.list(76, modifiers.clone().count(), 4)?;
    for (i, raw) in modifiers.enumerate() {
        data.word(
            start + 4 * i,
            unsigned(raw, "ResourceModifierValueList", path)?,
        );
    }
    for attr in node
        .attributes()
        .filter(|attr| !attr.value().trim().is_empty())
    {
        let raw = attr.value();
        if let Some(&(_, kind, offset)) = REFERENCES
            .iter()
            .find(|&&(field, _, _)| field == attr.name())
        {
            write_import(raw, kind, offset, &mut data, &mut result)?;
        } else if let Some(&(_, offset)) = WEAK_REFERENCES
            .iter()
            .find(|&&(field, _)| field == attr.name())
        {
            data.word(
                offset,
                weak_reference(raw, "GameObject", path, &mut result)?,
            );
        }
    }

    let mut children = significant_children(node).peekable();
    if let Some(alert) = children.next_if(|child| child.has_tag_name((SAGE_NS, "RedAlertTemplate")))
    {
        let id = weak_reference(
            text_value(alert, path)?,
            "RedAlertTemplate",
            path,
            &mut result,
        )?;
        data.optional_word(112, id)?;
    }
    for (tag, offset) in [
        ("PreferredColor", 116),
        ("PreferredCampaignRadarColor", 120),
    ] {
        data.word(
            offset,
            color(
                required_element(&mut children, tag, path)?,
                &["B", "G", "R"],
                255,
                path,
            )?,
        );
    }
    let upgrades = take_list(&mut children, "InitialUpgrade");
    write_reference_list(upgrades, "UpgradeTemplate", 124, &mut data, &mut result)?;

    let units = take_list(&mut children, "StartingUnitInfo");
    let start = data.list(132, units.len(), 16)?;
    for (i, unit) in units.into_iter().enumerate() {
        validate_attributes(unit, &[], path)?;
        let mut content = significant_children(unit);
        let object = required_element(&mut content, "Object", path)?;
        let position = required_element(&mut content, "Position", path)?;
        validate_leaf(position, &["x", "y", "z"], path)?;
        if content.next().is_some() {
            return Err(invalid(path, "unexpected content in StartingUnitInfo"));
        }
        let offset = start + 16 * i;
        write_import(
            text_value(object, path)?,
            "GameObject",
            offset,
            &mut data,
            &mut result,
        )?;
        for (i, axis) in ["x", "y", "z"].into_iter().enumerate() {
            data.word(
                offset + 4 * (i + 1),
                finite_number(position.attribute(axis).unwrap_or("0"), axis, path)?.to_bits(),
            );
        }
    }
    if let Some(filter) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ResourceModifierObjectFilter")))
    {
        let start = data.allocate(140, 112)?;
        object_filter(filter, start, &mut data)?;
        result
            .reference_types
            .extend(filter_reference_types(filter));
    }
    for (tag, offset) in [("StartingMoney", 144), ("MoneyCap", 152)] {
        let entries = take_list(&mut children, tag);
        let start = data.list(offset, entries.len(), 8)?;
        for (i, entry) in entries.into_iter().enumerate() {
            validate_leaf(entry, &["Account", "Amount"], path)?;
            for (j, field) in ["Account", "Amount"].into_iter().enumerate() {
                data.word(
                    start + 8 * i + 4 * j,
                    unsigned(required(entry, field, path)?, field, path)?,
                );
            }
        }
    }
    let beacons = take_list(&mut children, "AIBeaconSettings");
    let start = data.list(160, beacons.len(), 12)?;
    for (i, entry) in beacons.into_iter().enumerate() {
        validate_leaf(
            entry,
            &["BeaconType", "UnattachedTemplate", "AttachedTemplate"],
            path,
        )?;
        let offset = start + 12 * i;
        let kind = enum_index(
            required(entry, "BeaconType", path)?,
            &["Invalid", "Assault", "HoldPosition", "HoldObject"],
            "BeaconType",
            path,
        )?;
        data.word(offset, kind.wrapping_sub(1));
        for (field, displacement) in [("UnattachedTemplate", 4), ("AttachedTemplate", 8)] {
            if let Some(raw) = entry.attribute(field).filter(|raw| !raw.trim().is_empty()) {
                data.word(
                    offset + displacement,
                    weak_reference(raw, "GameObject", path, &mut result)?,
                );
            }
        }
    }
    let ranks = take_list(&mut children, "TechRank");
    write_reference_list(ranks, "PlayerTechRank", 168, &mut data, &mut result)?;

    let bonuses = take_list(&mut children, "TechProductionBonuses");
    let start = data.list(176, bonuses.len(), 28)?;
    const BONUS_FIELDS: [&str; 6] = [
        "BuildCostBonus",
        "BuildCostBonusAllies",
        "BuildTimeBonus",
        "BuildTimeBonusAllies",
        "HarvesterBonus",
        "HarvesterBonusAllies",
    ];
    let bonus_attrs = [
        "RequiredTech",
        BONUS_FIELDS[0],
        BONUS_FIELDS[1],
        BONUS_FIELDS[2],
        BONUS_FIELDS[3],
        BONUS_FIELDS[4],
        BONUS_FIELDS[5],
    ];
    for (i, bonus) in bonuses.into_iter().enumerate() {
        validate_leaf(bonus, &bonus_attrs, path)?;
        let offset = start + 28 * i;
        write_import(
            required(bonus, "RequiredTech", path)?,
            "PlayerTech",
            offset,
            &mut data,
            &mut result,
        )?;
        for (j, field) in BONUS_FIELDS.into_iter().enumerate() {
            data.word(
                offset + 4 * (j + 1),
                percentage(bonus.attribute(field).unwrap_or("0"), field, path)?.to_bits(),
            );
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in PlayerTemplate"));
    }
    if !data.relocations.is_empty() {
        data.relocations.sort_unstable();
        data.relocations.push(u32::MAX);
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

fn take_list<'a, 'input>(
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    tag: &str,
) -> Vec<Node<'a, 'input>> {
    std::iter::from_fn(|| children.next_if(|node| node.has_tag_name((SAGE_NS, tag)))).collect()
}

fn write_reference_list(
    entries: Vec<Node<'_, '_>>,
    kind: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let start = data.list(offset, entries.len(), 4)?;
    for (i, entry) in entries.into_iter().enumerate() {
        write_import(
            text_value(entry, data.path)?,
            kind,
            start + 4 * i,
            data,
            result,
        )?;
    }
    Ok(())
}

fn weak_reference(raw: &str, kind: &str, path: &Path, result: &mut Marshaled) -> io::Result<u32> {
    let name = raw.split_once('\\').map_or(raw, |(name, _)| name);
    let id = asset_reference(name, kind, path)?;
    result.reference_types.push(id.type_id);
    Ok(id.instance_id)
}

fn validate_leaf(node: Node<'_, '_>, attributes: &[&str], path: &Path) -> io::Result<()> {
    validate_attributes(node, attributes, path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            format!("unexpected content in {}", node.tag_name().name()),
        ));
    }
    Ok(())
}

fn unsigned(raw: &str, field: &str, path: &Path) -> io::Result<u32> {
    raw.trim()
        .parse()
        .map_err(|_| invalid(path, format!("invalid {field}")))
}
