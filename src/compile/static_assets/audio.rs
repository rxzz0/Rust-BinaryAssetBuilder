use super::binary::BinaryData;
use super::global_ui::enum_value;
use super::write_import;
use super::{
    Marshaled, asset_reference, finite_number, percentage, significant_children,
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
    let event = node.has_tag_name((SAGE_NS, "AudioEvent"));
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
    let mut ranges = [None; 4];
    let range_names = ["PitchShift", "PerFilePitchShift", "Delay", "InitialDelay"];
    let mut filename = None;
    let mut lists = [Vec::new(), Vec::new(), Vec::new()];
    for child in significant_children(node) {
        let name = child.tag_name().name();
        if child.tag_name().namespace() == Some(SAGE_NS) {
            if let Some(i) = range_names.iter().position(|&candidate| candidate == name) {
                if ranges[i].replace(child).is_none() {
                    continue;
                }
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
    if let Some(filename) = filename {
        let filename = super::text_value(filename, path)?;
        let offset = data.allocate(0x80, 4)?;
        audio_file(filename, offset, &mut data, &mut result)?;
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
            audio_file(filename, offset, &mut data, &mut result)?;
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
    for (index, range) in ranges.into_iter().enumerate() {
        let Some(range) = range else { continue };
        validate_attributes(range, &["Low", "High"], path)?;
        if has_content(range) {
            return Err(invalid(path, "unexpected audio range content"));
        }
        let start = data.allocate(0x54 + index * 4, 8)?;
        for (name, offset) in [("Low", start), ("High", start + 4)] {
            let value = required(range, name, path)?;
            let word = if index < 2 {
                finite_number(value, name, path)?.to_bits()
            } else {
                value
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, "invalid integer audio delay"))?
                    as u32
            };
            data.word(offset, word);
        }
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
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let value = value.split_once('\\').map_or(value, |(name, _)| name);
    let id = asset_reference(value, "AudioFile", data.path)?;
    result.imports.push(offset as u32);
    result.reference_types.push(id.type_id);
    result.references.push(id.into());
    data.word(offset, result.references.len() as u32);
    Ok(())
}
