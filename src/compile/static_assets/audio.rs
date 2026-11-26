use super::binary::BinaryData;
use super::global_ui::enum_value;
use super::write_import;
use super::{
    Marshaled, asset_reference, finite_number, percentage, sage_time, significant_children,
    validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

const PRIORITY: &[&str] = &["LOWEST", "LOW", "NORMAL", "HIGH", "HIGHEST", "CRITICAL"];
const TYPE: &[&str] = &[
    "UI", "WORLD", "SHROUDED", "VOICE", "PLAYER", "ALLIES", "ENEMIES", "EVERYONE",
];
const CONTROL: &[&str] = &[
    "LOOP",
    "SEQUENTIAL",
    "RANDOMSTART",
    "INTERRUPT",
    "SMART_LIMITING",
    "FADE_ON_KILL",
    "FADE_ON_START",
    "ALLOW_KILL_MID_FILE",
    "IMMEDIATE_DECAY_ON_KILL",
];
const SLIDER: &[&str] = &["SOUNDFX", "VOICE", "MUSIC", "AMBIENT", "MOVIE", "NONE"];

pub(super) fn single_sound(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const PERCENTAGES: &[(&str, &str, usize)] = &[
        ("Volume", "100", 4),
        ("VolumeShift", "0", 8),
        ("PerFileVolumeShift", "0", 0xc),
        ("MinVolume", "0", 0x10),
        ("ShrunkenVolumeModifier", "100", 0x18),
        ("PlayPercent", "100", 0x1c),
        ("LowPassCutoff", "0", 0x38),
        ("ZoomedInOffscreenVolumePercent", "50", 0x3c),
        ("ZoomedInOffscreenMinVolumePercent", "100", 0x40),
        ("ZoomedInOffscreenOcclusionPercent", "20", 0x44),
        ("ReverbEffectLevel", "0", 0x48),
        ("DryLevel", "100", 0x4c),
    ];
    const REALS: &[(&str, &str, usize)] = &[
        ("ShrunkenPitchModifier", "1", 0x14),
        ("MinRange", "160", 0x30),
        ("MaxRange", "640", 0x34),
    ];
    let allowed: Vec<_> = ["id", "Limit", "Priority", "Type", "Control", "SubmixSlider"]
        .into_iter()
        .chain(PERCENTAGES.iter().chain(REALS).map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let event = matches!(
        node.tag_name().name(),
        "AudioEvent" | "AudioEventOverridable"
    );
    let filename_type = match node.tag_name().name() {
        "MP3AmbientStream" | "MP3DialogEvent" | "MP3MusicTrack" => "AudioFileMP3Passthrough",
        _ => "AudioFile",
    };
    let mut data = BinaryData {
        payload: vec![0; if event { 0x98 } else { 0x84 }],
        relocations: Vec::new(),
        path,
    };
    for &(name, default, offset) in PERCENTAGES {
        data.word(
            offset,
            percentage(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    for &(name, default, offset) in REALS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    let limit = node
        .attribute("Limit")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid audio Limit"))?;
    data.word(0x20, limit as u32);
    data.word(
        0x24,
        enum_value(
            node.attribute("Priority").unwrap_or("NORMAL"),
            PRIORITY,
            path,
        )?,
    );
    for (name, options, offset) in [("Type", TYPE, 0x28), ("Control", CONTROL, 0x2c)] {
        let mut flags = 0;
        for value in node.attribute(name).unwrap_or("").split_ascii_whitespace() {
            flags |= 1 << enum_value(value, options, path)?;
        }
        data.word(offset, flags);
    }
    let mut result = Marshaled::default();
    let mut ranges = [None; 7];
    let range_names = [
        "PitchShift",
        "PerFilePitchShift",
        "Delay",
        "InitialDelay",
        "MinRangeShift",
        "MaxRangeShift",
        "NonInterruptibleTime",
    ];
    let mut multipliers = Vec::new();
    let mut groups = Vec::new();
    let mut filename = None;
    let mut lists = [Vec::new(), Vec::new(), Vec::new()];
    for child in significant_children(node) {
        let name = child.tag_name().name();
        if child.tag_name().namespace() == Some(SAGE_NS) {
            if let Some(i) = range_names.iter().position(|&candidate| candidate == name) {
                if ranges[i].replace(child).is_none() {
                    continue;
                }
            } else if name == "VolumeSliderMultiplier" {
                multipliers.push(child);
                continue;
            } else if name == "LimitGroup" {
                groups.push(child);
                continue;
            } else if event {
                if let Some(i) = ["Attack", "Sound", "Decay"]
                    .iter()
                    .position(|&candidate| candidate == name)
                {
                    lists[i].push(child);
                    continue;
                }
            } else if name == "Filename" && filename.replace(child).is_none() {
                continue;
            }
        }
        return Err(invalid(
            path,
            format!("unsupported {} child {name}", node.tag_name().name()),
        ));
    }
    // The reference table follows the base sound fields, while imports follow payload allocation.
    for &group in &groups {
        let value = super::text_value(group, path)?;
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
        let reference = asset_reference(value, "AudioEventLimitGroup", path)?;
        result.reference_types.push(reference.type_id);
        result.references.push(reference.into());
    }
    if let Some(filename) = filename {
        let filename = super::text_value(filename, path)?;
        let offset = data.allocate(0x80, 4)?;
        audio_file(filename, filename_type, offset, &mut data, &mut result)?;
    }
    for (index, list) in lists.into_iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let first = data.list(0x80 + index * 8, list.len(), 12)?;
        for (index, item) in list.into_iter().enumerate() {
            validate_attributes(item, &["Weight", "Volume"], path)?;
            let mut text = significant_children(item);
            let filename = text
                .next()
                .filter(Node::is_text)
                .and_then(|child| child.text());
            let Some(filename) = filename.filter(|_| text.next().is_none()) else {
                return Err(invalid(path, "expected audio file reference text"));
            };
            let offset = first + index * 12;
            audio_file(filename, filename_type, offset, &mut data, &mut result)?;
            let weight = item
                .attribute("Weight")
                .unwrap_or("1000")
                .trim()
                .parse()
                .map_err(|_| invalid(path, "invalid audio Weight"))?;
            data.word(offset + 4, weight);
            data.word(
                offset + 8,
                percentage(item.attribute("Volume").unwrap_or("100"), "Volume", path)?.to_bits(),
            );
        }
    }
    if let Some(slider) = node.attribute("SubmixSlider") {
        let index = enum_value(slider, SLIDER, path)?;
        let offset = data.allocate(0x50, 4)?;
        data.word(offset, index);
    }
    for (index, range) in ranges[..4].iter().enumerate() {
        if let Some(range) = range {
            audio_range(*range, 0x54 + index * 4, &mut data)?;
        }
    }
    let first = data.list(0x64, multipliers.len(), 8)?;
    for (index, item) in multipliers.into_iter().enumerate() {
        validate_attributes(item, &["Slider", "Multiplier"], path)?;
        if has_content(item) {
            return Err(invalid(path, "unexpected VolumeSliderMultiplier content"));
        }
        let offset = first + index * 8;
        data.word(
            offset,
            enum_value(required(item, "Slider", path)?, SLIDER, path)?,
        );
        let value = required(item, "Multiplier", path)?;
        data.word(
            offset + 4,
            finite_number(value, "Multiplier", path)?.to_bits(),
        );
    }
    for (range, pointer) in [(&ranges[4], 0x6c), (&ranges[5], 0x70)] {
        if let Some(range) = range {
            audio_range(*range, pointer, &mut data)?;
        }
    }
    let first = data.list(0x74, groups.len(), 4)?;
    for index in 0..groups.len() {
        let offset = first + index * 4;
        data.word(offset, index as u32 + 1);
        result.imports.push(offset as u32);
    }
    if let Some(range) = ranges[6] {
        audio_range(range, 0x7c, &mut data)?;
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    Ok(result)
}

fn audio_range(node: Node<'_, '_>, pointer: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let path = data.path;
    validate_attributes(node, &["Low", "High"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected audio range content"));
    }
    let start = data.allocate(pointer, 8)?;
    for (name, offset) in [("Low", start), ("High", start + 4)] {
        let value = required(node, name, path)?;
        let word = match pointer {
            0x5c | 0x60 => audio_integer(value, name, path)?,
            0x7c => sage_time(value, name, path)?.to_bits(),
            _ => finite_number(value, name, path)?.to_bits(),
        };
        data.word(offset, word);
    }
    Ok(())
}

fn audio_integer(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    let value = value.trim();
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else {
        value.parse::<i32>().map(|word| word as u32)
    };
    parsed.map_err(|_| invalid(path, format!("invalid audio {name}: {value}")))
}

pub(super) fn limit_group(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Limit"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected AudioEventLimitGroup content"));
    }
    let limit = audio_integer(required(node, "Limit", path)?, "Limit", path)?;
    Ok(Marshaled {
        payload: super::encode_words([0, limit]),
        ..Marshaled::default()
    })
}

pub(super) fn multisound(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Control"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 0x10],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let mut control = 0;
    for flag in node
        .attribute("Control")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        control |= 1 << enum_value(flag, &["LOOP", "PLAY_ONE"], path)?;
    }
    data.word(4, control);
    let sounds: Vec<_> = significant_children(node).collect();
    let first = data.list(8, sounds.len(), 0x1c)?;
    for (index, sound) in sounds.into_iter().enumerate() {
        if !sound.has_tag_name((SAGE_NS, "Subsound")) {
            return Err(invalid(path, "unexpected Multisound child"));
        }
        const OVERRIDES: [&str; 5] = [
            "PitchShiftLow",
            "PitchShiftHigh",
            "Volume",
            "PlayPercent",
            "VolumeShift",
        ];
        validate_attributes(
            sound,
            &[
                "Weight",
                "PitchShiftLow",
                "PitchShiftHigh",
                "Volume",
                "PlayPercent",
                "VolumeShift",
            ],
            path,
        )?;
        let mut content = significant_children(sound);
        let name = content
            .next()
            .filter(Node::is_text)
            .and_then(|child| child.text());
        let Some(name) = name.filter(|_| content.next().is_none()) else {
            return Err(invalid(path, "expected Subsound reference text"));
        };
        let offset = first + index * 0x1c;
        write_import(name, "BaseAudioEventInfo", offset, &mut data, &mut result)?;
        let weight = sound
            .attribute("Weight")
            .unwrap_or("1000")
            .trim()
            .parse()
            .map_err(|_| invalid(path, "invalid Subsound Weight"))?;
        data.word(offset + 4, weight);
        for (index, name) in OVERRIDES.into_iter().enumerate() {
            if let Some(raw) = sound.attribute(name) {
                let value = if index < 2 {
                    finite_number(raw, name, path)?
                } else {
                    percentage(raw, name, path)?
                };
                data.optional_word(offset + 8 + index * 4, value.to_bits())?;
            }
        }
    }
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

fn audio_file(
    value: &str,
    filename_type: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let value = value.split_once('\\').map_or(value, |(name, _)| name);
    let id = asset_reference(value, filename_type, data.path)?;
    result.imports.push(offset as u32);
    result.reference_types.push(id.type_id);
    result.references.push(id.into());
    data.word(offset, result.references.len() as u32);
    Ok(())
}
