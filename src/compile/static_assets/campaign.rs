use super::binary::BinaryData;
use super::{
    Marshaled, asset_reference, finite_number, hash_name, required_element, significant_children,
    text_value, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn mission(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const STRINGS: [&str; 8] = [
        "DisplayName",
        "ShortDisplayName",
        "OverviewTalkingHead",
        "OverviewBackground",
        "Title",
        "MapName",
        "BriefingFLA",
        "BriefingSummary",
    ];
    const ACHIEVEMENTS: [&str; 5] = [
        "VictoryAchEvent",
        "VictoryAchEventMedium",
        "VictoryAchEventHard",
        "VictoryAchEventCoop",
        "VictoryAchEventBonusObj",
    ];
    let attributes: Vec<_> = std::iter::once("id")
        .chain(STRINGS)
        .chain(ACHIEVEMENTS)
        .collect();
    validate_attributes(node, &attributes, path)?;
    let mut data = Marshaled::default();
    let mut binary = layout(148, path);
    strings(&mut binary, node, 4, &STRINGS)?;
    for (index, field) in ACHIEVEMENTS.into_iter().enumerate() {
        let value = node
            .attribute(field)
            .unwrap_or("0")
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, format!("invalid mission {field}")))?;
        binary.word(68 + index * 4, value);
    }

    let mut children = significant_children(node).peekable();
    let prerequisites = take_children(&mut children, "Prerequisite");
    let list = binary.list(88, prerequisites.len(), 4)?;
    for (index, entry) in prerequisites.into_iter().enumerate() {
        validate_attributes(entry, &[], path)?;
        if entry.children().any(|child| child.is_element()) {
            return Err(invalid(path, "invalid mission Prerequisite"));
        }
        let value = entry
            .text()
            .unwrap_or("-1")
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, "invalid mission Prerequisite"))?;
        binary.word(list + 4 * index, value as u32);
    }
    for (tag, offset) in [("Objective", 96), ("BonusObjective", 104)] {
        let entries = take_children(&mut children, tag);
        let list = binary.list(offset, entries.len(), 8)?;
        for (index, entry) in entries.into_iter().enumerate() {
            binary.string(list + 8 * index, text_value(entry, path)?)?;
        }
    }
    let commanders = take_children(&mut children, "AICommander");
    if !(1..=3).contains(&commanders.len()) {
        return Err(invalid(
            path,
            "a mission requires one to three AI commanders",
        ));
    }
    let list = binary.list(112, commanders.len(), 24)?;
    for (index, commander) in commanders.into_iter().enumerate() {
        let offset = list + 24 * index;
        validate_attributes(
            commander,
            &["Portrait", "Personality", "Description", "Faction"],
            path,
        )?;
        if has_content(commander) {
            return Err(invalid(path, "unexpected content in AICommander"));
        }
        if let Some(portrait) = commander
            .attribute("Portrait")
            .filter(|value| !value.trim().is_empty())
        {
            import(
                &mut binary,
                &mut data,
                offset,
                portrait,
                "OnDemandTextureImage",
            )?;
        }
        strings(
            &mut binary,
            commander,
            offset + 4,
            &["Personality", "Description"],
        )?;
        let faction = match required(commander, "Faction", path)? {
            "RANDOM" => u32::MAX,
            "Allies" => 0,
            "Soviet" => 1,
            "Japan" => 2,
            "Neutral" => 3,
            _ => return Err(invalid(path, "invalid AICommander Faction")),
        };
        binary.word(offset + 20, faction);
    }
    movie(
        &mut binary,
        &mut data,
        120,
        required_element(&mut children, "IntroMovie", path)?,
    )?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in MissionTemplate"));
    }
    Ok(finish(binary, data))
}

pub(super) fn campaign(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &["id", "DisplayName", "ConsoleAutosaveFilename"],
        path,
    )?;
    let mut data = Marshaled::default();
    let mut binary = layout(84, path);
    strings(
        &mut binary,
        node,
        4,
        &["DisplayName", "ConsoleAutosaveFilename"],
    )?;
    let mut children = significant_children(node).peekable();
    let missions = take_children(&mut children, "Mission");
    if missions.is_empty() {
        return Err(invalid(path, "a campaign requires a Mission"));
    }
    let list = binary.list(20, missions.len(), 4)?;
    for (index, mission) in missions.into_iter().enumerate() {
        import(
            &mut binary,
            &mut data,
            list + 4 * index,
            text_value(mission, path)?,
            "MissionTemplate",
        )?;
    }
    for (offset, name) in [(28, "FinalMovie"), (56, "AlternateFinalMovie")] {
        movie(
            &mut binary,
            &mut data,
            offset,
            required_element(&mut children, name, path)?,
        )?;
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in CampaignTemplate"));
    }
    Ok(finish(binary, data))
}

pub(super) fn map_metadata(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut binary = layout(12, path);
    let maps: Vec<_> = significant_children(node).collect();
    if maps.is_empty() {
        return Err(invalid(path, "expected a MapMetaData entry"));
    }
    let list = binary.list(4, maps.len(), 60)?;
    for (index, map) in maps.into_iter().enumerate() {
        if !map.has_tag_name((SAGE_NS, "MapMetaData")) {
            return Err(invalid(path, "unexpected MapMetaData child"));
        }
        validate_attributes(
            map,
            &[
                "id",
                "Description",
                "DisplayName",
                "NumPlayers",
                "CRC",
                "FileName",
                "Width",
                "Height",
                "BorderSize",
                "IsMultiplayer",
                "IsOfficial",
            ],
            path,
        )?;
        let offset = list + 60 * index;
        if let Some(id) = map.attribute("id") {
            binary.word(offset, hash_name(id));
        }
        strings(
            &mut binary,
            map,
            offset + 4,
            &["Description", "DisplayName"],
        )?;
        binary.word(offset + 20, signed(map, "NumPlayers", path)?);
        let crc = map
            .attribute("CRC")
            .unwrap_or("0")
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, "invalid map CRC"))?;
        binary.word(offset + 24, crc);
        strings(&mut binary, map, offset + 28, &["FileName"])?;
        for (word, name) in [(36, "Width"), (40, "Height"), (44, "BorderSize")] {
            binary.word(offset + word, signed(map, name, path)?);
        }
        let starts: Vec<_> = significant_children(map).collect();
        if starts.len() < 2 {
            return Err(invalid(
                path,
                "MapMetaData requires at least two StartPosition entries",
            ));
        }
        let positions = binary.list(offset + 48, starts.len(), 20)?;
        for (index, start) in starts.into_iter().enumerate() {
            if !start.has_tag_name((SAGE_NS, "StartPosition")) {
                return Err(invalid(path, "expected a StartPosition"));
            }
            validate_attributes(start, &["Name"], path)?;
            let entry = positions + 20 * index;
            strings(&mut binary, start, entry, &["Name"])?;
            let mut children = significant_children(start);
            let position = required_element(&mut children, "Position", path)?;
            validate_attributes(position, &["x", "y", "z"], path)?;
            if children.next().is_some() || has_content(position) {
                return Err(invalid(path, "unexpected StartPosition content"));
            }
            for (index, axis) in ["x", "y", "z"].into_iter().enumerate() {
                let value = finite_number(required(position, axis, path)?, axis, path)?;
                binary.word(entry + 8 + 4 * index, value.to_bits());
            }
        }
        let multiplayer = boolean(map.attribute("IsMultiplayer").unwrap_or("false"), path)?;
        let official = boolean(map.attribute("IsOfficial").unwrap_or("false"), path)?;
        binary.word(offset + 56, multiplayer | (official << 8));
    }
    Ok(finish(binary, Marshaled::default()))
}

fn take_children<'a, 'input, I: Iterator<Item = Node<'a, 'input>>>(
    children: &mut std::iter::Peekable<I>,
    name: &str,
) -> Vec<Node<'a, 'input>> {
    let mut nodes = Vec::new();
    while let Some(node) = children.next_if(|node| node.has_tag_name((SAGE_NS, name))) {
        nodes.push(node);
    }
    nodes
}

fn layout(size: usize, path: &Path) -> BinaryData<'_> {
    BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    }
}

fn strings(
    binary: &mut BinaryData<'_>,
    node: Node<'_, '_>,
    offset: usize,
    names: &[&str],
) -> io::Result<()> {
    for (index, name) in names.iter().enumerate() {
        if let Some(text) = node.attribute(*name) {
            binary.string(offset + 8 * index, text)?;
        }
    }
    Ok(())
}

fn movie(
    binary: &mut BinaryData<'_>,
    data: &mut Marshaled,
    offset: usize,
    node: Node<'_, '_>,
) -> io::Result<()> {
    validate_attributes(
        node,
        &["ImageRef", "FileName", "ScreenName", "Description"],
        binary.path,
    )?;
    if has_content(node) || node.attribute("FileName").is_none() {
        return Err(invalid(
            binary.path,
            "expected a campaign movie with a FileName",
        ));
    }
    if let Some(image) = node
        .attribute("ImageRef")
        .filter(|value| !value.trim().is_empty())
    {
        import(binary, data, offset, image, "PackedTextureImage")?;
    }
    strings(
        binary,
        node,
        offset + 4,
        &["FileName", "ScreenName", "Description"],
    )
}

fn import(
    binary: &mut BinaryData<'_>,
    data: &mut Marshaled,
    offset: usize,
    value: &str,
    asset_type: &str,
) -> io::Result<()> {
    let id = asset_reference(value, asset_type, binary.path)?;
    data.references.push(id.into());
    data.reference_types.push(id.type_id);
    binary.word(offset, data.references.len() as u32);
    data.imports.push(offset as u32);
    Ok(())
}

fn finish(mut binary: BinaryData<'_>, mut data: Marshaled) -> Marshaled {
    for offsets in [&mut binary.relocations, &mut data.imports] {
        if !offsets.is_empty() {
            offsets.sort_unstable();
            offsets.push(u32::MAX);
        }
    }
    data.payload = binary.payload;
    data.relocations = binary.relocations;
    data
}

fn signed(node: Node<'_, '_>, name: &str, path: &Path) -> io::Result<u32> {
    node.attribute(name)
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map(|value| value as u32)
        .map_err(|_| invalid(path, format!("invalid map {name}")))
}

fn boolean(value: &str, path: &Path) -> io::Result<u32> {
    match value.trim() {
        "true" | "1" => Ok(1),
        "false" | "0" => Ok(0),
        _ => Err(invalid(path, "invalid map metadata boolean")),
    }
}
