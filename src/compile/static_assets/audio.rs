use super::binary::BinaryData;
use super::global_ui::enum_value;
use super::{
    Marshaled, asset_reference, finite_number, percentage, significant_children,
    validate_attributes,
};
use crate::compile::{SAGE_NS, invalid};
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

pub(super) fn ambient_stream(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
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
    let mut data = BinaryData {
        payload: vec![0; 0x84],
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
    let mut children = significant_children(node);
    let mut result = Marshaled::default();
    if let Some(filename) = children.next() {
        if !filename.has_tag_name((SAGE_NS, "Filename")) || children.next().is_some() {
            return Err(invalid(path, "unsupported AmbientStream child"));
        }
        let filename = super::text_value(filename, path)?;
        let id = asset_reference(filename, "AudioFile", path)?;
        let offset = data.allocate(0x80, 4)?;
        data.word(offset, 1);
        result.imports.extend([offset as u32, u32::MAX]);
        result.reference_types.push(id.type_id);
        result.references.push(id.into());
    }
    if let Some(slider) = node.attribute("SubmixSlider") {
        let index = enum_value(slider, SLIDER, path)?;
        let offset = data.allocate(0x50, 4)?;
        data.word(offset, index);
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}
