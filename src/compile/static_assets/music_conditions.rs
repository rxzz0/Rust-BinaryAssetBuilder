use super::binary::BinaryData;
use super::game_object::{encode_inline_object_filter, write_model_conditions};
use super::music::{finish_music_script, script_string_hash};
use super::{
    Marshaled, asset_reference, finite_number, sage_int, sage_time, significant_children,
    text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(in crate::compile) const BASE: &str = "MusicScriptConditionNuggetBase";

const COMPARISONS: &[&str] = &[
    "LESS_THAN",
    "LESS_THAN_OR_EQUAL",
    "EQUAL",
    "GREATER_THAN_OR_EQUAL",
    "GREATER_THAN",
];
const RELATIONSHIPS: &[&str] = &["ALLIES", "ENEMIES", "NEUTRAL", "SAME_PLAYER"];

#[derive(Clone, Copy)]
enum Value {
    Int,
    Time,
    Real,
    Bool,
    Hash,
    Enum(&'static [&'static str]),
    Flags(&'static [&'static str]),
    ModelFlags,
    Weak(&'static str),
    Strong(&'static str),
    OptionalStrong(&'static str),
}

use Value::*;

type Field = (&'static str, usize, Value, Option<&'static str>);

#[derive(Clone, Copy)]
enum Children {
    None,
    Filter(usize),
    Not,
    List,
}

struct Layout {
    name: &'static str,
    hash: u32,
    size: usize,
    fields: &'static [Field],
    children: Children,
}

macro_rules! condition {
    ($name:ident, $hash:expr, $size:expr, $children:expr, $fields:expr) => {
        Layout {
            name: concat!("MusicScriptConditionNugget_", stringify!($name)),
            hash: $hash,
            size: $size,
            children: $children,
            fields: $fields,
        }
    };
}

const INTERVAL: Field = ("TimeBetweenConditionChecks", 4, Time, Some("5s"));
const STATE: Field = ("TrackedState", 4, Weak("PathMusicGameDynamicState"), None);
const LAYOUTS: &[Layout] = &[
    condition!(AnyTrackPlaying, 0x6838_852b, 4, Children::None, &[]),
    condition!(
        LocalPlayerIsObserver,
        0x6bed_52e8,
        8,
        Children::None,
        &[("CountDeadPlayersAsObservers", 4, Bool, None),]
    ),
    condition!(
        TimeFromStartOfLevel,
        0x450b_816f,
        8,
        Children::None,
        &[("Timeout", 4, Time, None),]
    ),
    condition!(
        TrackPlayedCount,
        0xef2a_cd90,
        12,
        Children::None,
        &[
            ("Track", 4, Weak("MusicScriptTrack"), None),
            ("Count", 8, Int, None),
        ]
    ),
    condition!(
        SpecificTrackTypePlaying,
        0xe597_bf56,
        8,
        Children::None,
        &[("TrackType", 4, Hash, None),]
    ),
    condition!(
        UnitsFarFromBase,
        0xd02b_9da4,
        20,
        Children::None,
        &[
            INTERVAL,
            ("MinUnitsToPass", 8, Int, None),
            ("MinDistanceFromBase", 12, Real, None),
            ("Relationship", 16, Flags(RELATIONSHIPS), None),
        ]
    ),
    condition!(
        ObjectsOfTypeExist,
        0xa964_84c1,
        136,
        Children::Filter(132),
        &[
            INTERVAL,
            ("Count", 8, Int, Some("1")),
            ("RequiredModelConditions", 12, ModelFlags, Some("")),
            ("ExcludedModelConditions", 72, ModelFlags, Some("")),
        ]
    ),
    condition!(
        ObjectsNearEvaEvent,
        0x74b9_cd49,
        24,
        Children::Filter(20),
        &[
            INTERVAL,
            ("EvaEvent", 8, Strong("EvaEvent"), None),
            ("Count", 12, Int, Some("1")),
            ("Distance", 16, Real, None),
        ]
    ),
    condition!(
        EvaEventPlayedRecently,
        0x6fca_4d26,
        12,
        Children::None,
        &[
            ("EvaEvent", 4, Strong("EvaEvent"), None),
            ("Timeout", 8, Time, Some("1s")),
        ]
    ),
    condition!(
        ScoredKillCount,
        0xc2be_8d14,
        20,
        Children::None,
        &[
            ("Count", 4, Int, None),
            ("Time", 8, Time, None),
            ("Filter", 12, OptionalStrong("ObjectFilterAsset"), Some("")),
            ("CountOnlyKillsAgainstTheLocalPlayer", 16, Bool, None),
            ("CountOnlyKillsByTheLocalPlayer", 17, Bool, None),
        ]
    ),
    condition!(
        EngagementCount,
        0xb5cb_c912,
        28,
        Children::None,
        &[
            INTERVAL,
            ("Count", 8, Int, None),
            ("Time", 12, Time, None),
            (
                "RequiredScreenStatus",
                16,
                Enum(&["ON_SCREEN", "OFF_SCREEN", "ON_OR_OFF_SCREEN"]),
                None
            ),
            (
                "TargetFilter",
                20,
                OptionalStrong("ObjectFilterAsset"),
                Some("")
            ),
            ("CountOnlyEngagementsAgainstTheLocalPlayer", 24, Bool, None),
            ("CountOnlyEngagementsByTheLocalPlayer", 25, Bool, None),
        ]
    ),
    condition!(
        HitBySuperWeapon,
        0x243c_c246,
        24,
        Children::None,
        &[
            ("TrackedWeapon", 4, Weak("WeaponTemplate"), None),
            ("Time", 8, Time, None),
            ("Count", 12, Int, None),
            (
                "TargetFilter",
                16,
                OptionalStrong("ObjectFilterAsset"),
                Some("")
            ),
            ("CountOnlyHitsAgainstTheLocalPlayer", 20, Bool, None),
        ]
    ),
    condition!(
        LocalPlayerKillRatio,
        0xc567_6fed,
        20,
        Children::None,
        &[
            INTERVAL,
            ("Time", 8, Time, None),
            ("Ratio", 12, Real, None),
            ("RatioOp", 16, Enum(COMPARISONS), None),
        ]
    ),
    condition!(
        PathMusicStateCurrentlyActive,
        0xc9d7_c134,
        8,
        Children::None,
        &[STATE]
    ),
    condition!(
        PathMusicStateLastActive,
        0x8c0a_b66f,
        20,
        Children::None,
        &[
            STATE,
            ("TrackedStateChange", 8, Enum(&["ENTER", "EXIT"]), None),
            ("TimeSinceLastActive", 12, Time, None),
            ("TimeSicneLastActiveOp", 16, Enum(COMPARISONS), None),
        ]
    ),
    condition!(Not, 0x3305_5a8e, 8, Children::Not, &[]),
    condition!(Or, 0xf2e2_e91d, 12, Children::List, &[]),
    condition!(And, 0x1fba_d33f, 12, Children::List, &[]),
];

pub(in crate::compile) fn types() -> impl Iterator<Item = &'static str> {
    LAYOUTS.iter().map(|layout| layout.name)
}

fn layout(name: &str) -> Option<&'static Layout> {
    LAYOUTS.iter().find(|layout| layout.name == name)
}

pub(super) fn type_hash(name: &str) -> Option<u32> {
    layout(name).map(|layout| layout.hash)
}

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let layout = layout(node.tag_name().name()).expect("registered music condition");
    let names: Vec<_> = std::iter::once("id")
        .chain(layout.fields.iter().map(|field| field.0))
        .collect();
    validate_attributes(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; layout.size],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for &(name, offset, kind, default) in layout.fields {
        if matches!(kind, Weak(_) | Strong(_) | OptionalStrong(_)) {
            if default.is_none() {
                required(node, name, path)?;
            }
            continue;
        }
        let value = match default {
            Some(default) => node.attribute(name).unwrap_or(default),
            None => required(node, name, path)?,
        };
        let word = match kind {
            Int => sage_int(value, name, path)?,
            Time => sage_time(value, name, path)?.to_bits(),
            Real => finite_number(value, name, path)?.to_bits(),
            Hash => script_string_hash(value, name, path)?,
            Enum(values) => super::global_ui::enum_value(value, values, path)?,
            Flags(values) => super::ai_strategic::flags(value, values, name, path)?,
            Bool => {
                data.payload[offset] = super::fx_list::boolean(node, name, false, path)?;
                continue;
            }
            ModelFlags => {
                write_model_conditions(value, offset, &mut data)?;
                continue;
            }
            Weak(_) | Strong(_) | OptionalStrong(_) => unreachable!(),
        };
        data.word(offset, word);
    }
    // Reference type hashes follow XML attribute order.
    for attribute in node.attributes() {
        let Some(&(_, offset, kind, _)) = layout
            .fields
            .iter()
            .find(|field| field.0 == attribute.name())
        else {
            continue;
        };
        let value = attribute.value();
        match kind {
            Weak(target) => {
                let id = asset_reference(value, target, path)?;
                result.reference_types.push(id.type_id);
                data.word(offset, id.instance_id);
            }
            Strong(target) => write_import(value, target, offset, &mut data, &mut result)?,
            OptionalStrong(target) if !value.trim().is_empty() => {
                let start = data.allocate(offset, 4)?;
                write_import(value, target, start, &mut data, &mut result)?;
            }
            _ => {}
        }
    }
    let children: Vec<_> = significant_children(node).collect();
    match (layout.children, children.as_slice()) {
        (Children::None | Children::Filter(_), []) => {}
        (Children::Filter(offset), [filter]) if filter.has_tag_name((SAGE_NS, "Filter")) => {
            let start = data.allocate(offset, 112)?;
            encode_inline_object_filter(*filter, start, &mut data, &mut result.reference_types)?;
        }
        (Children::Not, [condition]) => {
            write_child_reference(*condition, 4, &mut data, &mut result)?;
        }
        (Children::List, [_, _, ..]) => {
            let start = data.list(4, children.len(), 4)?;
            for (index, condition) in children.into_iter().enumerate() {
                write_child_reference(condition, start + index * 4, &mut data, &mut result)?;
            }
        }
        _ => {
            return Err(invalid(
                path,
                format!("unexpected children in {}", layout.name),
            ));
        }
    }
    Ok(finish_music_script(result, data))
}

fn write_child_reference(
    condition: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    if !condition.has_tag_name((SAGE_NS, "Condition")) {
        return Err(invalid(data.path, "expected a music script Condition"));
    }
    write_import(
        text_value(condition, data.path)?,
        BASE,
        offset,
        data,
        result,
    )
}
