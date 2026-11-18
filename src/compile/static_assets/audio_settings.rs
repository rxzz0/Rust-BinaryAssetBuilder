use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::global_ui::enum_value;
use super::{
    Marshaled, encode_words, finite_number, percentage, sage_int, sage_time, sage_uint,
    significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

type Parser = fn(&str, &str, &Path) -> io::Result<u32>;
type Field = (&'static str, &'static str, Parser);

fn real(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    Ok(finite_number(value, name, path)?.to_bits())
}

fn percent(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    Ok(percentage(value, name, path)?.to_bits())
}

fn seconds(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    Ok(sage_time(value, name, path)?.to_bits())
}

fn priority(value: &str, _: &str, path: &Path) -> io::Result<u32> {
    enum_value(
        value,
        &["LOWEST", "LOW", "NORMAL", "HIGH", "HIGHEST", "CRITICAL"],
        path,
    )
}

const SETTINGS: &[Field] = &[
    ("StreamBufferSizePerChannel", "65536", sage_int),
    ("MaxRequestsPerStream", "4", sage_int),
    ("ForceResetTime", "0s", seconds),
    ("EmergencyResetTime", "0s", seconds),
    ("MinOcclusion", "0", percent),
    ("DelayPriorToPlayingToReadSoundFile", "1s", seconds),
    ("ReadAheadTime", "3s", seconds),
    ("QueueAheadTime", ".5s", seconds),
    ("LongTime", "60s", seconds),
    ("AudioFootprintInBytes", "4194304", sage_uint),
    (
        "DontLoadThirdVariationWhenFileReadQueueIsLongerThan",
        "50",
        sage_uint,
    ),
    (
        "DontLoadSecondVariationWhenFileReadQueueIsLongerThan",
        "100",
        sage_uint,
    ),
    (
        "DontLoadForLowPrioritySoundsWhenFileReadQueueIsLongerThan",
        "100",
        sage_uint,
    ),
    (
        "WhatIsLowPriorityForDontLoadForLowPrioritySoundsWhenFileReadQueueIsLongerThan",
        "LOW",
        priority,
    ),
    ("VolumeToGainConversionFactor", "1.66667", real),
    ("DefaultSoundVolume", "100", percent),
    ("DefaultVoiceVolume", "100", percent),
    ("DefaultMusicVolume", "100", percent),
    ("DefaultMovieVolume", "100", percent),
    ("DefaultAmbientVolume", "100", percent),
    ("AutomaticSubtitleDuration", "8s", seconds),
    ("AutomaticSubtitleLines", "8", sage_int),
    ("PanRadiusScaleValue", "1.0", real),
    ("PanSize", "1.0", real),
    ("MinSampleVolume", "0.01", percent),
    ("PositionDeltaForReverbCheck", "10.0", real),
    ("TimeToFadeAudio", "1s", seconds),
    ("AmbientStreamHysteresisVolume", "5", sage_int),
    ("VolumeMultiplierFor2DSounds", "100%", percent),
    ("VolumeMultiplierFor3DSounds", "100%", percent),
    ("VoiceMoveToCampMaxCampnessAtStartPoint", "0", sage_uint),
    ("VoiceMoveToCampMinCampnessAtEndPoint", "5", sage_uint),
    ("MinDelayBetweenEnterStateVoice", "5s", seconds),
    ("TimeSinceLastAttackForVoiceRetreatToCastle", "3s", seconds),
    (
        "DistanceToLookForEnemiesForVoiceRetreatToCastle",
        "500",
        real,
    ),
    ("PostPostGameMusicWait", "10s", seconds),
    ("CapturedUnitsVoicePriorityOffset", "0", sage_int),
    ("UISelectionVoicePriorityOffset", "0", sage_int),
    ("AddToSelectionVoiceTimeout", "0s", seconds),
    ("MinVolumeBeforeFade", "10", percent),
    ("MaxVolumeBeforeFade", "20", percent),
    ("PathMusicBufferTime", "1.5s", seconds),
    ("PathMusicLatencyMilisec", "500", sage_int),
];

const MICROPHONE: &[Field] = &[
    ("MicrophonePreferredFractionCameraToGround", "86", percent),
    (
        "VolumeMicrophonePullTowardsTerrainLookAtPoint",
        "60",
        percent,
    ),
    (
        "PanningMicrophonePullTowardsTerrainLookAtPoint",
        "60",
        percent,
    ),
    ("MicrophoneMinDistanceToCamera", "100", real),
    ("MicrophoneMaxDistanceToCamera", "300", real),
    ("ZoomMinDistance", "130", real),
    ("ZoomMaxDistance", "425", real),
    ("ZoomSoundVolumePercentageAmount", "20", percent),
    ("ZoomFadeDistanceForMaxEffect", "200", real),
    ("ZoomFadeZeroEffectEdgeLength", "260", real),
    ("ZoomFadeFullEffectEdgeLength", "135", real),
];

const COMPRESSOR: &[Field] = &[
    ("Threshold", "20", real),
    ("Ratio", "10", real),
    ("AttackTime", "100ms", seconds),
    ("ReleaseTime", "500ms", seconds),
];

fn field_names(fields: &[Field], extra: &[&'static str]) -> Vec<&'static str> {
    fields
        .iter()
        .map(|field| field.0)
        .chain(extra.iter().copied())
        .collect()
}

fn write_fields(
    node: Node<'_, '_>,
    base: usize,
    fields: &[Field],
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for (index, &(name, default, parse)) in fields.iter().enumerate() {
        let value = parse(node.attribute(name).unwrap_or(default), name, data.path)?;
        data.word(base + index * 4, value);
    }
    Ok(())
}

pub(super) fn settings(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let flags = [
        "PlayEnemySightedEventsOnlyFromNearbyUnits",
        "SuppressOcclusion",
    ];
    validate_attributes(
        node,
        &field_names(SETTINGS, &["id", flags[0], flags[1]]),
        path,
    )?;
    let mut data = BinaryData {
        payload: vec![0; 384],
        relocations: Vec::new(),
        path,
    };
    write_fields(node, 4, SETTINGS, &mut data)?;
    for (index, name) in flags.into_iter().enumerate() {
        data.payload[380 + index] = boolean(node, name, false, path)?;
    }

    let singles = [
        "AutomaticSubtitleWindowColor",
        "AutomaticSubtitleTextColor",
        "TacticalMicSettings",
        "LivingWorldMicSettings",
        "ShellMicSettings",
        "VolumeCompressorSettings",
    ];
    let mut sections = [None; 6];
    let mut sliders = Vec::new();
    for child in significant_children(node) {
        let tag = child.tag_name();
        match tag.name() {
            "VolumeSliderMultiplierForMovie" if tag.namespace() == Some(SAGE_NS) => {
                sliders.push(child);
            }
            name if tag.namespace() == Some(SAGE_NS) => {
                let index = singles.iter().position(|&key| key == name).ok_or_else(|| {
                    invalid(path, format!("unexpected AudioSettings element {name}"))
                })?;
                if sections[index].replace(child).is_some() {
                    return Err(invalid(
                        path,
                        format!("duplicate AudioSettings element {name}"),
                    ));
                }
            }
            name => {
                return Err(invalid(
                    path,
                    format!("unexpected AudioSettings element {name}"),
                ));
            }
        }
    }
    let [
        Some(window),
        Some(text),
        Some(tactical),
        Some(world),
        Some(shell),
        Some(compressor),
    ] = sections
    else {
        return Err(invalid(path, "missing required AudioSettings element"));
    };
    for (color, offset) in [(window, 176), (text, 192)] {
        let fields = ["R", "G", "B", "A"];
        validate_leaf(color, &fields, path)?;
        for (index, name) in fields.into_iter().enumerate() {
            data.word(
                offset + index * 4,
                real(required(color, name, path)?, name, path)?,
            );
        }
    }
    let slider_base = data.list(208, sliders.len(), 8)?;
    for (index, slider) in sliders.into_iter().enumerate() {
        validate_leaf(slider, &["Slider", "Multiplier"], path)?;
        let offset = slider_base + index * 8;
        let slider_type = enum_value(
            required(slider, "Slider", path)?,
            &["SOUNDFX", "VOICE", "MUSIC", "AMBIENT", "MOVIE", "NONE"],
            path,
        )?;
        data.word(offset, slider_type);
        data.word(
            offset + 4,
            real(required(slider, "Multiplier", path)?, "Multiplier", path)?,
        );
    }
    let mic_fields = field_names(MICROPHONE, &["id"]);
    for (microphone, offset) in [(tactical, 220), (world, 268), (shell, 316)] {
        validate_leaf(microphone, &mic_fields, path)?;
        required(microphone, "id", path)?;
        write_fields(microphone, offset, MICROPHONE, &mut data)?;
    }
    const CHANNELS: &str = "AppliesEquallyToAllChannels";
    validate_leaf(compressor, &field_names(COMPRESSOR, &[CHANNELS]), path)?;
    write_fields(compressor, 360, COMPRESSOR, &mut data)?;
    data.payload[376] = boolean(compressor, CHANNELS, true, path)?;
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn animation(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const DISTANCE: &str = "MinMicrophoneDistanceToDirty";
    validate_leaf(node, &["id", DISTANCE], path)?;
    Ok(Marshaled {
        payload: encode_words([
            0,
            sage_int(node.attribute(DISTANCE).unwrap_or("0"), DISTANCE, path)?,
        ]),
        ..Marshaled::default()
    })
}

pub(super) fn lod(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let flags = ["AllowReverb", "AllowCPULimiter", "AllowVolumeCompressor"];
    const COUNTS: &[(&str, Parser)] = &[
        ("MaximumAmbientStreams", sage_int),
        ("CPULimiterLevel", percent),
        ("CPULimiterLevelDebug", percent),
        ("MaxVoices", sage_int),
        ("OutputBitRate", sage_int),
        ("MinimumLogicalProcessors", sage_uint),
    ];
    let names: Vec<_> = ["id", "Level"]
        .into_iter()
        .chain(COUNTS.iter().map(|(name, _)| *name))
        .chain(flags)
        .collect();
    validate_attributes(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 44],
        relocations: Vec::new(),
        path,
    };
    let level = enum_value(
        required(node, "Level", path)?,
        &["UNKNOWN", "LOW", "HIGH"],
        path,
    )?;
    data.word(4, level.wrapping_sub(1));
    for (index, &(name, parse)) in COUNTS.iter().enumerate() {
        data.word(
            8 + index * 4,
            parse(required(node, name, path)?, name, path)?,
        );
    }
    for (index, name) in flags.into_iter().enumerate() {
        required(node, name, path)?;
        data.payload[40 + index] = boolean(node, name, false, path)?;
    }
    let processors: Vec<_> = significant_children(node).collect();
    let start = data.list(32, processors.len(), 8)?;
    for (index, processor) in processors.into_iter().enumerate() {
        if !processor.has_tag_name((SAGE_NS, "Processor")) {
            return Err(invalid(path, "expected an AudioLOD Processor"));
        }
        validate_leaf(processor, &["Type", "MinMHz"], path)?;
        let kind = enum_value(
            required(processor, "Type", path)?,
            &["UNKNOWN", "PENTIUM_4", "CORE_DUO", "ATHLON", "ATHLON_64"],
            path,
        )?;
        let offset = start + index * 8;
        data.word(offset, kind);
        data.word(
            offset + 4,
            sage_int(required(processor, "MinMHz", path)?, "MinMHz", path)?,
        );
    }
    Ok(data.finish(Marshaled::default()))
}
