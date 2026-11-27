use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::game_object::weak_xml_reference;
use super::{
    Marshaled, asset_reference, sage_time, significant_children, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn settings(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const SCAN: &str = "EnemySightedMaxVoicePositionScanRange";
    const KEEP: &str = "MaxTimeToKeepJumpToEvents";
    const RESET: &str = "MaxTimeBeforeResettingLastJumpTo";
    const DISTANCE: &str = "MinDistanceBetweenJumpToEvents";
    validate_leaf(node, &["id", SCAN, KEEP, RESET, DISTANCE], path)?;
    let mut data = BinaryData {
        payload: vec![0; 0x14],
        relocations: Vec::new(),
        path,
    };
    for (name, offset) in [(SCAN, 4), (DISTANCE, 0x10)] {
        let value = node.attribute(name).unwrap_or("0").trim();
        let value = if let Some(value) = value.strip_prefix("0x") {
            u32::from_str_radix(value, 16)
        } else {
            value.parse::<i32>().map(|value| value as u32)
        }
        .map_err(|_| invalid(path, format!("invalid {name}")))?;
        data.word(offset, value);
    }
    for (name, offset) in [(KEEP, 8), (RESET, 0xc)] {
        let value = sage_time(node.attribute(name).unwrap_or("0s"), name, path)?;
        data.word(offset, value.to_bits());
    }
    Ok(Marshaled {
        payload: data.payload,
        ..Marshaled::default()
    })
}

pub(super) fn event(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const TIMES: [(&str, &str); 4] = [
        ("TimeBetweenEvents", "0s"),
        ("ExpirationTime", "2s"),
        ("QuietTime", "0s"),
        ("TimeToWaitBeforePlaying", "0s"),
    ];
    const FLAGS: [(&str, bool); 4] = [
        ("AlwaysPlayFromHomeBase", false),
        ("CountAsJumpToLocation", true),
        ("SuppressOn360", false),
        ("SuppressOnPC", false),
    ];
    let allowed: Vec<_> = ["id", "Priority"]
        .into_iter()
        .chain(TIMES.map(|(name, _)| name))
        .chain(FLAGS.map(|(name, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut data = BinaryData {
        payload: vec![0; 0x2c],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    let priority = node
        .attribute("Priority")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid EVA Priority"))?;
    data.word(4, priority);
    for (index, (name, default)) in TIMES.into_iter().enumerate() {
        let value = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(8 + index * 4, value.to_bits());
    }
    for (index, (name, default)) in FLAGS.into_iter().enumerate() {
        data.payload[0x28 + index] = boolean(node, name, default, path)?;
    }

    let mut children = significant_children(node).peekable();
    let mut sounds = Vec::new();
    while let Some(sound) = children.next_if(|node| node.has_tag_name((SAGE_NS, "SideSound"))) {
        sounds.push(sound);
    }
    let sounds_start = data.list(0x18, sounds.len(), 8)?;
    for (index, sound) in sounds.into_iter().enumerate() {
        validate_leaf(sound, &["Sound", "Side"], path)?;
        let offset = sounds_start + index * 8;
        for attribute in sound.attributes() {
            match attribute.name() {
                "Sound" => write_import(
                    attribute.value(),
                    "BaseAudioEventInfo",
                    offset,
                    &mut data,
                    &mut out,
                )?,
                "Side" => {
                    let value = attribute.value();
                    let value = value.split_once('\\').map_or(value, |(name, _)| name);
                    let side = asset_reference(value, "PlayerTemplate", path)?;
                    data.word(offset + 4, side.instance_id);
                    out.reference_types.push(side.type_id);
                }
                _ => {}
            }
        }
        if sound.attribute("Sound") != Some("") {
            required(sound, "Sound", path)?;
        }
        required(sound, "Side", path)?;
    }

    let mut blocked = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, "OtherEvaEventToBlock")) {
            return Err(invalid(path, "unexpected EvaEvent child"));
        }
        blocked.push(weak_xml_reference(
            child,
            "EvaEvent",
            &data,
            &mut out.reference_types,
        )?);
    }
    let first = data.list(0x20, blocked.len(), 4)?;
    for (index, id) in blocked.into_iter().enumerate() {
        data.word(first + index * 4, id);
    }

    for offsets in [&mut data.relocations, &mut out.imports] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}
