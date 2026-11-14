use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::{
    Marshaled, fast_hash, finite_number, sage_time, significant_children, text_value,
    validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path, sync::OnceLock};

pub(in crate::compile::static_assets) fn compile(
    node: Node<'_, '_>,
    path: &Path,
) -> io::Result<Marshaled> {
    const TIMES: [(&str, usize); 2] = [("ReloadTime", 0x20), ("ViewObjectDuration", 0x2c)];
    const REALS: [(&str, usize, &str); 6] = [
        ("ViewObjectRange", 0x30, "0"),
        ("RadiusCursorRadius", 0x34, "0"),
        ("MaxCastRange", 0xe4, "1000000"),
        ("MinCastRange", 0xe8, "0"),
        ("ForbiddenObjectRange", 0xec, "0"),
        ("ReachableTargetsCircleDisplayRadius", 0x130, "0"),
    ];
    const STATUS: [(&str, usize); 4] = [
        ("PreventConditions", 0x38),
        ("RequiredConditions", 0x54),
        ("RequiredTargetObjectStatus", 0xac),
        ("DisallowedTargetObjectStatus", 0xc8),
    ];
    const REFS: [(&str, usize, &str); 9] = [
        ("InitiateSound", 0x24, ""),
        ("InitiateAtLocationSound", 0x28, ""),
        ("EvaEventToPlayOnSuccess", 0xf0, "EvaEvent"),
        ("EvaEventToPlayWhenSelectingTarget", 0xf4, "EvaEvent"),
        ("EvaEventToPlayOnInitiateOwner", 0xf8, "EvaEvent"),
        ("EvaEventToPlayOnInitiateAlly", 0xfc, "EvaEvent"),
        ("EvaEventToPlayOnInitiateEnemy", 0x100, "EvaEvent"),
        ("TimerImage", 0x12c, "PackedTextureImage"),
        ("RequiredPlayerTech", 0x134, "PlayerTech"),
    ];
    const VOICES: [&str; 8] = [
        "NameOfVoiceNameToUseAsInitiateIntendToDoVoice",
        "NameOfVoiceNameToUseAsInitiateIntendToDoAfterMoveVoice",
        "NameOfVoiceNameToUseAsInitiateIntendToDoWhileUnderAttackVoice",
        "NameOfVoiceNameToUseAsInitiateIntendToDoAfterMoveWhileUnderAttackVoice",
        "NameOfVoiceNameToUseAsEnterStateInitiateIntendToDoVoice",
        "NameOfVoiceNameToUseAsEnterStateInitiateIntendToDoAfterMoveVoice",
        "NameOfVoiceNameToUseAsEnterStateInitiateIntendToDoWhileUnderAttackVoice",
        "NameOfVoiceNameToUseAsEnterStateInitiateIntendToDoAfterMoveWhileUnderAttackVoice",
    ];
    let allowed: Vec<_> = [
        "id",
        "DisplayName",
        "Description",
        "TargetType",
        "NumTargets",
        "Flags",
        "DisallowedTargetModelStates",
        "RestrictionType",
        "Money",
        "WaypointModeTerminal",
    ]
    .into_iter()
    .chain(TIMES.map(|(n, _)| n))
    .chain(REALS.map(|(n, ..)| n))
    .chain(STATUS.map(|(n, _)| n))
    .chain(REFS.map(|(n, ..)| n))
    .chain(VOICES)
    .collect();
    validate_attributes(node, &allowed, path)?;
    let mut data = BinaryData {
        payload: vec![0; 0x228],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    for (name, offset) in [("DisplayName", 4), ("Description", 0xc)] {
        if let Some(value) = node.attribute(name) {
            data.string(offset, value)?;
        }
    }
    data.word(
        0x14,
        enum_value(
            required(node, "TargetType", path)?,
            &[
                "LOCATION",
                "LOCATION_AND_ANGLE",
                "OBJECT",
                "OBJECT_OR_LOCATION",
                "NONE",
            ],
            path,
        )?,
    );
    for (name, offset, default) in [("NumTargets", 0x18, "1"), ("Money", 0x128, "0")] {
        let value = node
            .attribute(name)
            .unwrap_or(default)
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, format!("invalid {name}")))?;
        data.word(offset, value as u32);
    }
    static FLAGS: OnceLock<Vec<String>> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        super::ai_update::enum_values(
            include_str!("../../../../schemas/ra3/xsd/AssetTypeSpecialPowerTemplate.xsd"),
            "SpecialPowerTemplateFlag",
        )
    });
    let mut bits = 0u32;
    for value in node
        .attribute("Flags")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        let bit = flags
            .iter()
            .position(|flag| flag == value)
            .filter(|&i| i < 32)
            .ok_or_else(|| invalid(path, format!("invalid special power flag {value}")))?;
        bits |= 1 << bit;
    }
    data.word(0x1c, bits);
    for (name, offset) in TIMES {
        data.word(
            offset,
            sage_time(node.attribute(name).unwrap_or("0s"), name, path)?.to_bits(),
        );
    }
    for (name, offset, default) in REALS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    for (name, offset) in STATUS {
        super::secondary_damage::write_status_flags(
            node.attribute(name).unwrap_or(""),
            offset,
            &mut data,
        )?;
    }
    super::draws::write_model_conditions(
        node.attribute("DisallowedTargetModelStates").unwrap_or(""),
        0x70,
        &mut data,
    )?;
    for attr in node.attributes() {
        let Some(&(_, offset, kind)) = REFS.iter().find(|&&(name, ..)| name == attr.name()) else {
            continue;
        };
        let ordinal = if kind.is_empty() {
            super::audio_reference(
                attr.value(),
                path,
                &mut out.references,
                &mut out.reference_types,
            )?
        } else {
            super::passive_area::reference(
                attr.value(),
                kind,
                path,
                &mut out.references,
                &mut out.reference_types,
            )?
        };
        if ordinal != 0 {
            data.word(offset, ordinal);
            out.imports.push(offset as u32);
        }
    }
    for (index, name) in VOICES.into_iter().enumerate() {
        if let Some(value) = node.attribute(name) {
            data.optional_word(0x104 + index * 4, fast_hash(value.as_bytes()))?;
        }
    }
    data.word(
        0x124,
        enum_value(
            node.attribute("RestrictionType").unwrap_or("UNRESTRICTED"),
            &["UNRESTRICTED", "1", "2"],
            path,
        )?,
    );
    data.payload[0x224] = super::draws::boolean(node, "WaypointModeTerminal", true, path)?;
    let mut children = significant_children(node).peekable();
    for (name, offset) in [("ObjectFilter", 0x138), ("ForbiddenObjectFilter", 0x1a8)] {
        if let Some(filter) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            super::parking::encode_inline_filter(
                filter,
                offset,
                &mut data,
                &mut out.reference_types,
            )?;
        }
    }
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "GameDependency"))) {
        super::children::dependency(child, 0x218, &mut data, &mut out.reference_types)?;
    }
    let techs: Vec<_> = children.collect();
    let start = data.list(0x21c, techs.len(), 4)?;
    for (index, child) in techs.into_iter().enumerate() {
        if !child.has_tag_name((SAGE_NS, "ForbiddenPlayerTech")) {
            return Err(invalid(path, "unexpected special power child"));
        }
        let ordinal = super::passive_area::reference(
            text_value(child, path)?,
            "PlayerTech",
            path,
            &mut out.references,
            &mut out.reference_types,
        )?;
        let offset = start + index * 4;
        data.word(offset, ordinal);
        out.imports.push(offset as u32);
    }
    data.relocations.sort_unstable();
    out.imports.sort_unstable();
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    if !out.imports.is_empty() {
        out.imports.push(u32::MAX);
    }
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}
