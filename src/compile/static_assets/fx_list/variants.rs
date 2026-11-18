use super::{BinaryData, Marshaled, SAGE_NS, enum_value, import, validate_leaf, vector};
use crate::compile::static_assets::{
    asset_reference, finite_number, percentage, sage_angle, sage_time,
};
use crate::compile::{invalid, required};
use roxmltree::Node;
use std::io;

#[derive(Clone, Copy)]
enum Value {
    Real(&'static str),
    Time(&'static str),
    Percentage,
    Angle,
    Signed,
    Unsigned,
    Enum(&'static [&'static str], Option<&'static str>),
    Terrain,
    Flag(bool),
    String,
    Asset(&'static str),
    WeakObject,
}

use Value::*;

type Attribute = (&'static str, usize, Value);

#[derive(Clone, Copy)]
enum Child {
    Vector3,
    Vector2,
    Rgb,
    PackedRgb,
    TerrainRange,
}
use Child::*;

pub(super) struct Layout {
    pub(super) kind: u32,
    pub(super) size: usize,
    attributes: &'static [Attribute],
    children: &'static [(&'static str, usize, Child)],
}

const VIEW_SHAKE: &[&str] = &[
    "SUBTLE",
    "NORMAL",
    "STRONG",
    "SEVERE",
    "CINE_EXTREME",
    "CINE_INSANE",
];
const TERRAIN: &[&str] = &[
    "RANDOM",
    "SCORCH_1",
    "SCORCH_2",
    "SCORCH_3",
    "SCORCH_4",
    "SCORCH_5",
    "SCORCH_6",
    "SCORCH_7",
    "SCORCH_8",
    "SCORCH_9",
    "SCORCH_10",
    "SCORCH_11",
    "SCORCH_12",
    "SCORCH_13",
    "SCORCH_14",
    "SCORCH_15",
    "SCORCH_16",
    "TERRAIN_MAPPED",
];

pub(super) fn layout(tag: &str) -> Option<Layout> {
    let (kind, size, attributes, children): (_, _, &[_], &[_]) = match tag {
        "LightPulse" => (
            0xffe7_5ca1,
            0x48,
            &[
                ("Radius", 0x2c, Real("0")),
                ("RadiusAsPercentOfObjectSize", 0x30, Percentage),
                ("IncreaseTime", 0x34, Unsigned),
                ("DecreaseTime", 0x38, Unsigned),
            ],
            &[("Color", 0x3c, Rgb)],
        ),
        "DynamicDecal" => (
            0x33a6_dcee,
            0x6c,
            &[
                ("Decal", 0x2c, String),
                ("Shader", 0x34, Enum(&["ALPHA", "ADDITIVE"], Some("ALPHA"))),
                ("Size", 0x38, Real("0")),
                ("OpacityStart", 0x3c, Real("0")),
                ("OpacityPeak", 0x40, Real("1")),
                ("OpacityEnd", 0x44, Real("0")),
                ("OpacityFadeTimeOne", 0x48, Time("0s")),
                ("OpacityFadeTimeTwo", 0x4c, Time("0s")),
                ("OpacityPeakTime", 0x50, Time("0s")),
                ("StartingDelay", 0x54, Time("0s")),
                ("Lifetime", 0x58, Time("0s")),
                ("OrientToObject", 0x68, Flag(true)),
                ("AllowOnObjects", 0x69, Flag(true)),
            ],
            &[("Color", 0x5c, PackedRgb), ("Offset", 0x60, Vector2)],
        ),
        "Laser" => (
            0x35e9_3221,
            0x40,
            &[
                ("LaserTemplate", 0x2c, WeakObject),
                ("LaserBackwards", 0x3c, Flag(false)),
            ],
            &[("Offset", 0x30, Vector3)],
        ),
        "CameraShakerVolume" => (
            0xe0a2_31e5,
            0x38,
            &[
                ("Radius", 0x2c, Real("0")),
                ("Duration", 0x30, Time("0s")),
                ("Amplitude", 0x34, Angle),
            ],
            &[],
        ),
        "ViewShake" => (
            0xef18_1572,
            0x30,
            &[("Type", 0x2c, Enum(VIEW_SHAKE, None))],
            &[],
        ),
        "AttachedModel" => (
            0x86e8_845f,
            0x38,
            &[
                ("Model", 0x2c, Asset("BaseRenderAssetType")),
                ("ExpireTimer", 0x30, Time("8s")),
                ("RandomlyRotate", 0x34, Flag(false)),
            ],
            &[],
        ),
        "TerrainScorch" => (
            0x083c_afa6,
            0x38,
            &[("Type", 0x2c, Terrain), ("Radius", 0x30, Real("0"))],
            &[("RandomRange", 0x34, TerrainRange)],
        ),
        "TintDrawable" => (
            0x7d92_6e58,
            0x4c,
            &[
                ("PreColorTime", 0x2c, Time("2s")),
                ("PostColorTime", 0x30, Time("2s")),
                ("SustainedColorTime", 0x34, Time("1s")),
                ("Frequency", 0x38, Real("1")),
                ("Amplitude", 0x3c, Real("1")),
            ],
            &[("Color", 0x40, Rgb)],
        ),
        "FXListAtBonePos" => (
            0xfc64_0e01,
            0x38,
            &[("FX", 0x2c, Asset("FXList")), ("Bone", 0x30, String)],
            &[],
        ),
        "FXParticleSysBone" => (
            0xba0b_1771,
            0x54,
            &[
                ("Bone", 0x2c, String),
                (
                    "TriggerType",
                    0x34,
                    Enum(&["NONE", "CATAPULT_ROCK", "TREBUCHET_ROCK"], Some("NONE")),
                ),
                ("HoldBetweenStateID", 0x38, Signed),
                (
                    "ActionType",
                    0x3c,
                    Enum(&["NONE", "HOLD", "KILL", "SPAWN"], Some("NONE")),
                ),
                ("Particle", 0x40, Asset("FXParticleSystemTemplate")),
                ("RequireFrequentUpdate", 0x50, Flag(false)),
            ],
            &[("Offset", 0x44, Vector3)],
        ),
        _ => return None,
    };
    Some(Layout {
        kind,
        size,
        attributes,
        children,
    })
}

impl Layout {
    pub(super) fn attributes(&self) -> impl Iterator<Item = &'static str> {
        self.attributes.iter().map(|&(name, ..)| name)
    }

    pub(super) fn encode<'a, 'input: 'a>(
        &self,
        node: Node<'a, 'input>,
        start: usize,
        children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
        data: &mut BinaryData<'_>,
        out: &mut Marshaled,
    ) -> io::Result<()> {
        let path = data.path;
        for &(name, offset, ty) in self.attributes {
            let at = start + offset;
            let raw = node.attribute(name);
            let word = match ty {
                Real(default) => finite_number(raw.unwrap_or(default), name, path)?.to_bits(),
                Time(default) => sage_time(raw.unwrap_or(default), name, path)?.to_bits(),
                Percentage => percentage(raw.unwrap_or("0%"), name, path)?.to_bits(),
                Angle => sage_angle(raw.unwrap_or("0d"), name, path)?.to_bits(),
                Signed => super::integer(node, name, 0, path)?,
                Unsigned => raw
                    .unwrap_or("0")
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?,
                Enum(choices, default) => {
                    let raw = match default {
                        Some(default) => raw.unwrap_or(default),
                        None => required(node, name, path)?,
                    };
                    enum_value(raw, choices, path)?
                }
                Terrain => enum_value(raw.unwrap_or("RANDOM"), TERRAIN, path)?.wrapping_sub(1),
                Flag(default) => {
                    data.payload[at] = super::boolean(node, name, default, path)?;
                    continue;
                }
                String => {
                    if let Some(value) = raw.filter(|value| !value.is_empty()) {
                        data.string(at, value)?;
                    }
                    continue;
                }
                Asset(kind) => {
                    import(raw, kind, at, data, out)?;
                    continue;
                }
                WeakObject => {
                    if let Some(value) = raw.filter(|value| !value.trim().is_empty()) {
                        let reference = asset_reference(value, "GameObject", path)?;
                        out.reference_types.push(reference.type_id);
                        data.word(at, reference.instance_id);
                    }
                    continue;
                }
            };
            data.word(at, word);
        }
        for &(name, offset, ty) in self.children {
            let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) else {
                continue;
            };
            let at = start + offset;
            match ty {
                Vector3 => vector(child, false, at, data)?,
                Vector2 => {
                    validate_leaf(child, &["x", "y"], path)?;
                    for (axis, name) in ["x", "y"].into_iter().enumerate() {
                        data.word(
                            at + axis * 4,
                            finite_number(child.attribute(name).unwrap_or("0"), name, path)?
                                .to_bits(),
                        );
                    }
                }
                Rgb => {
                    validate_leaf(child, &["R", "G", "B"], path)?;
                    for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
                        data.word(
                            at + channel * 4,
                            finite_number(required(child, name, path)?, name, path)?.to_bits(),
                        );
                    }
                }
                PackedRgb => {
                    validate_leaf(child, &["R", "G", "B"], path)?;
                    let mut color = [0, 0, 0, 255];
                    for (byte, name) in color.iter_mut().zip(["B", "G", "R"]) {
                        *byte = child
                            .attribute(name)
                            .unwrap_or("0")
                            .trim()
                            .parse::<u8>()
                            .map_err(|_| invalid(path, format!("invalid {name} channel")))?;
                    }
                    data.word(at, u32::from_le_bytes(color));
                }
                TerrainRange => {
                    validate_leaf(child, &["Low", "High"], path)?;
                    let target = data.allocate(at, 8)?;
                    for (name, offset, default) in [("Low", 0, "SCORCH_1"), ("High", 4, "SCORCH_4")]
                    {
                        let ordinal =
                            enum_value(child.attribute(name).unwrap_or(default), TERRAIN, path)?;
                        data.word(target + offset, ordinal.wrapping_sub(1));
                    }
                }
            }
        }
        Ok(())
    }
}
