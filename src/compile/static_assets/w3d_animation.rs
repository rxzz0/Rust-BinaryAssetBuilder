mod delta;
mod timecoded;

use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::{
    Marshaled, finite_number, required_element, significant_children, validate_attributes,
    write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

const CHANNEL_KINDS: &[&str] = &[
    "XTranslation",
    "YTranslation",
    "ZTranslation",
    "Orientation",
    "Visibility",
];

#[derive(Clone, Copy)]
struct Frame {
    position: usize,
    values: [f32; 4],
    binary_move: bool,
}

struct CompressionSettings {
    max_translation_error: f32,
    max_rotation_error: f32,
    max_visibility_error: f32,
    max_adaptive_delta_error: f32,
    force_reduction_rate: f32,
    allow_time_coded: bool,
    allow_adaptive_delta: bool,
}

impl CompressionSettings {
    fn read(node: Option<Node<'_, '_>>, path: &Path) -> io::Result<Self> {
        if let Some(node) = node {
            validate_leaf(
                node,
                &[
                    "MaxTranslationError",
                    "MaxRotationError",
                    "MaxVisibilityError",
                    "MaxAdaptiveDeltaError",
                    "ForceReductionRate",
                    "AllowTimeCoded",
                    "AllowAdaptiveDelta",
                ],
                path,
            )?;
            let real =
                |name, default| finite_number(node.attribute(name).unwrap_or(default), name, path);
            let allow_time_coded = boolean(node, "AllowTimeCoded", true, path)? != 0;
            let allow_adaptive_delta = boolean(node, "AllowAdaptiveDelta", true, path)? != 0;
            let use_exact_timecodes = !allow_time_coded && !allow_adaptive_delta;
            return Ok(Self {
                max_translation_error: if use_exact_timecodes {
                    0.0
                } else {
                    real("MaxTranslationError", "0.001")?
                },
                max_rotation_error: if use_exact_timecodes {
                    0.0
                } else {
                    real("MaxRotationError", "0.01")?
                },
                max_visibility_error: if use_exact_timecodes {
                    0.0
                } else {
                    real("MaxVisibilityError", "0.01")?
                },
                max_adaptive_delta_error: real("MaxAdaptiveDeltaError", "0.03")?,
                force_reduction_rate: if use_exact_timecodes {
                    1.0
                } else {
                    real("ForceReductionRate", "1")?
                },
                allow_time_coded: allow_time_coded || use_exact_timecodes,
                allow_adaptive_delta,
            });
        }
        Ok(Self {
            max_translation_error: 0.002,
            max_rotation_error: 0.003,
            max_visibility_error: 0.01,
            max_adaptive_delta_error: 0.001,
            force_reduction_rate: 1.0,
            allow_time_coded: true,
            allow_adaptive_delta: true,
        })
    }
}

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Hierarchy", "NumFrames", "FrameRate"], path)?;
    let num_frames = unsigned(node, "NumFrames", path)?;
    let frame_rate = unsigned(node, "FrameRate", path)?;
    let mut children = significant_children(node).peekable();
    let settings = children.next_if(|child| child.has_tag_name((SAGE_NS, "CompressionSettings")));
    let settings = CompressionSettings::read(settings, path)?;
    let channels = required_element(&mut children, "Channels", path)?;
    validate_attributes(channels, &[], path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected W3DAnimation child"));
    }
    let channels: Vec<_> = significant_children(channels).collect();
    let mut data = BinaryData {
        payload: vec![0; 0x18],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    if let Some(hierarchy) = node
        .attribute("Hierarchy")
        .filter(|name| !name.trim().is_empty())
    {
        write_import(hierarchy, "W3DHierarchy", 4, &mut data, &mut out)?;
        out.imports.push(u32::MAX);
    }
    data.word(8, num_frames);
    data.word(0xc, frame_rate);
    data.word(0x10, channels.len() as u32);
    let pointers = data.allocate(
        0x14,
        channels
            .len()
            .checked_mul(4)
            .ok_or_else(|| invalid(path, "too many W3DAnimation channels"))?,
    )?;
    let mut channel_relocations = Vec::with_capacity(channels.len());
    for (index, channel) in channels.into_iter().enumerate() {
        let (kind, pivot, width, frames) = read_channel(channel, num_frames as usize, path)?;
        let pointer = pointers + index * 4;
        let before = data.relocations.len();
        compress_channel(&frames, kind, width, &settings)
            .encode(kind, pivot, pointer, &mut data)?;
        channel_relocations.push(data.relocations.remove(before));
    }
    data.relocations.splice(1..1, channel_relocations);
    data.relocations.push(u32::MAX);
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}

enum CompressedChannel {
    Delta(delta::Delta),
    Timecoded(timecoded::Timecoded),
}

impl CompressedChannel {
    fn encode(
        self,
        kind: u32,
        pivot: u16,
        pointer: usize,
        data: &mut BinaryData<'_>,
    ) -> io::Result<()> {
        match self {
            Self::Delta(channel) => channel.encode(kind, pivot, pointer, data),
            Self::Timecoded(channel) => channel.encode(kind, pivot, pointer, data),
        }
    }
}

fn compress_channel(
    frames: &[Frame],
    kind: u32,
    width: usize,
    settings: &CompressionSettings,
) -> CompressedChannel {
    let delta = settings
        .allow_adaptive_delta
        .then(|| delta::compress(frames, kind, width, settings))
        .flatten();
    let timecoded = (delta.is_none() || settings.allow_time_coded)
        .then(|| timecoded::compress(frames, kind, width, settings));
    match (timecoded, delta) {
        (Some(timecoded), Some(delta)) if timecoded.estimate_size() < delta.estimate_size() => {
            CompressedChannel::Timecoded(timecoded)
        }
        (_, Some(delta)) => CompressedChannel::Delta(delta),
        (Some(timecoded), None) => CompressedChannel::Timecoded(timecoded),
        (None, None) => unreachable!("timecoded compression is always available"),
    }
}

fn read_channel(
    node: Node<'_, '_>,
    frame_count: usize,
    path: &Path,
) -> io::Result<(u32, u16, usize, Vec<Frame>)> {
    let width = match (node.tag_name().namespace(), node.tag_name().name()) {
        (Some(SAGE_NS), "ChannelScalar") => 1,
        (Some(SAGE_NS), "ChannelQuaternion") => 4,
        _ => return Err(invalid(path, "unexpected W3DAnimation channel")),
    };
    validate_attributes(node, &["Pivot", "Type", "FirstFrame"], path)?;
    let kind = super::ai_strategic::enum_index(
        node.attribute("Type").unwrap_or("XTranslation"),
        CHANNEL_KINDS,
        "animation channel Type",
        path,
    )?;
    if width == 1 && kind == 3 {
        return Err(invalid(path, "scalar channel cannot encode Orientation"));
    }
    let pivot = u16::try_from(unsigned(node, "Pivot", path)?)
        .map_err(|_| invalid(path, "animation channel Pivot exceeds 16 bits"))?;
    let first = unsigned(node, "FirstFrame", path)? as usize;
    let mut children = significant_children(node);
    let samples = children.by_ref().count();
    let end = first
        .checked_add(samples)
        .filter(|&end| samples != 0 && end <= frame_count)
        .ok_or_else(|| invalid(path, "channel frame range exceeds W3DAnimation NumFrames"))?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(frame_count)
        .map_err(|_| invalid(path, "W3DAnimation frame data is too large"))?;
    let mut values = [0.0; 4];
    if width == 4 {
        values[3] = 1.0;
    } else if kind == 4 {
        values[0] = 1.0;
    }
    frames.extend((0..frame_count).map(|position| Frame {
        position,
        values,
        binary_move: false,
    }));
    for (frame, child) in frames[first..end]
        .iter_mut()
        .zip(significant_children(node))
    {
        if !child.has_tag_name((SAGE_NS, "Frame")) {
            return Err(invalid(path, "expected W3DAnimation Frame"));
        }
        frame.binary_move = boolean(child, "BinaryMove", false, path)? != 0;
        if width == 4 {
            const FIELDS: [&str; 5] = ["X", "Y", "Z", "W", "BinaryMove"];
            validate_leaf(child, &FIELDS, path)?;
            for (index, name) in FIELDS[..4].iter().copied().enumerate() {
                frame.values[index] = finite_number(required(child, name, path)?, name, path)?;
            }
        } else {
            validate_attributes(child, &["BinaryMove"], path)?;
            let mut children = significant_children(child);
            let text = children
                .next()
                .and_then(|node| node.text().filter(|_| node.is_text()))
                .filter(|_| children.next().is_none())
                .ok_or_else(|| invalid(path, "expected scalar frame value"))?;
            frame.values[0] = finite_number(text, "Frame", path)?;
        }
    }
    Ok((kind, pivot, width, frames))
}

fn unsigned(node: Node<'_, '_>, name: &str, path: &Path) -> io::Result<u32> {
    node.attribute(name)
        .unwrap_or("0")
        .trim()
        .parse()
        .map_err(|_| invalid(path, format!("invalid W3DAnimation {name}")))
}
