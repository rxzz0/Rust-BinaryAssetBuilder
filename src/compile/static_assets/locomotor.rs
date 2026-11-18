use super::ai_strategic::{enum_index, flags};
use super::binary::BinaryData;
use super::fx_list::boolean;
use super::game_object::{write_model_conditions, write_status_flags};
use super::{
    Marshaled, SAGE_NS, finite_number, has_content, invalid, percentage, sage_angle, sage_time,
    significant_children, text_value, validate_attributes, write_import,
};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const LOCOMOTOR_SCHEMA: &str =
    include_str!("../../../schemas/ra3/xsd/AssetTypeLocomotorTemplate.xsd");
const TERRAIN_SCHEMA: &str = include_str!("../../../schemas/ra3/xsd/AssetTypeTerrainAsset.xsd");
const BOUNCE_MAPPING: &str = "BounceKickTerrainMapping";
const ROOT_SIZE: usize = 0x18c;
const BOUNCE_POINTER: usize = 0x168;
const JET_POINTER: usize = 0x16c;
const MODEL_CONDITION_BYTES: usize = 0x3c;
const OBJECT_STATUS_BYTES: usize = 0x1c;

// Byte offsets in the RA3 runtime record. Read field types, defaults, and enum order
// from the bundled schema.
const FIELD_OFFSETS: &[(&str, usize)] = &[
    ("Surfaces", 0x004),
    ("LookAheadMult", 0x008),
    ("MaxSpeedDamaged", 0x00c),
    ("MinSpeed", 0x010),
    ("TurnTimeSeconds", 0x014),
    ("TurnTimeDamagedSeconds", 0x018),
    ("SlowTurnRadius", 0x01c),
    ("FastTurnRadius", 0x020),
    ("MinRandomTurnRadiusScale", 0x024),
    ("MaxRandomTurnRadiusScale", 0x028),
    ("TurnThreshold", 0x02c),
    ("TurnThresholdHS", 0x030),
    ("AccelerationSeconds", 0x034),
    ("Lift", 0x038),
    ("LiftDamaged", 0x03c),
    ("BrakingSeconds", 0x040),
    ("MinTurnSpeed", 0x044),
    ("PreferredHeight", 0x048),
    ("PreferredAttackHeight", 0x04c),
    ("PreferredHeightDamping", 0x050),
    ("PreferredHeightPitchingEpsilon", 0x054),
    ("CirclingRadius", 0x058),
    ("CirclingSpeed", 0x05c),
    ("BehaviorZ", 0x060),
    ("Appearance", 0x064),
    ("FormationPriority", 0x068),
    ("ActiveModelConditions", 0x06c),
    ("ActiveObjectStatus", 0x070),
    ("EnteringModelConditions", 0x074),
    ("EnteringModelConditionsTime", 0x078),
    ("AccDecTrigger", 0x07c),
    ("WalkDistance", 0x080),
    ("MaxTurnWithoutReform", 0x084),
    ("AccelPitchLimit", 0x088),
    ("BounceKick", 0x08c),
    ("PitchStiffness", 0x090),
    ("RollStiffness", 0x094),
    ("PitchDamping", 0x098),
    ("RollDamping", 0x09c),
    ("PitchByZVelCoef", 0x0a0),
    ("YawStiffness", 0x0a4),
    ("YawDamping", 0x0a8),
    ("MaxOverlappedHeight", 0x0ac),
    ("ForwardVelocityPitchFactor", 0x0b0),
    ("LateralVelocityRollFactor", 0x0b4),
    ("ForwardAccelerationPitchFactor", 0x0b8),
    ("LateralAccelerationRollFactor", 0x0bc),
    ("UniformAxialDamping", 0x0c0),
    ("TurnPivotOffset", 0x0c4),
    ("AirborneTargetingHeight", 0x0c8),
    ("CloseEnoughDist", 0x0cc),
    ("ReverseMoveSpeed", 0x0d0),
    ("MaximumWheelExtension", 0x0d4),
    ("MaximumWheelCompression", 0x0d8),
    ("WheelTurnAngle", 0x0dc),
    ("WanderWidthFactor", 0x0e0),
    ("WanderLengthFactor", 0x0e4),
    ("WanderAboutPointRadius", 0x0e8),
    ("BurniningDeathRadius", 0x0ec),
    ("ChargeMaxSpeed", 0x0f0),
    ("RudderCorrectionDegree", 0x0f4),
    ("RudderCorrectionRate", 0x0f8),
    ("ElevatorCorrectionDegree", 0x0fc),
    ("ElevatorCorrectionRate", 0x100),
    ("AeleronCorrectionDegree", 0x104),
    ("AeleronCorrectionRate", 0x108),
    ("SwoopStandoffRadius", 0x10c),
    ("SwoopStandoffHeight", 0x110),
    ("SwoopTerminalVelocity", 0x114),
    ("SwoopAccelerationRate", 0x118),
    ("SwoopSpeedTuningFactor", 0x11c),
    ("BackingUpSpeed", 0x120),
    ("BackingUpDistanceMin", 0x124),
    ("BackingUpDistanceMax", 0x128),
    ("BackingUpAngle", 0x12c),
    ("RiverModifier", 0x130),
    ("TakeOffAndLandingSpeed", 0x134),
    ("TakeOffAndLandingSlowDownDelta", 0x138),
    ("TakeOffAndLandingSlowDownTime", 0x13c),
    ("EasingPercentage", 0x140),
    ("AttackPathTrailDistance", 0x144),
    ("AttackPathTrailDistanceMinScale", 0x148),
    ("AttackPathTrailDistanceMaxScale", 0x14c),
    ("AbsoluteMinHeightWorldSpace", 0x150),
    ("SpeedBasedHeightOffset", 0x154),
    ("ResubmergeDelay", 0x158),
    ("WaterToAirTransitionFX", 0x15c),
    ("WaterSurfaceHeightOffset", 0x160),
    ("ForbiddenObjectStatus", 0x164),
    ("MakeTransformNonDirty", 0x170),
    ("IsCloseEnoughDist3D", 0x171),
    ("DontNegateDeceleratePitchFactor", 0x172),
    ("LocomotorWorksWhenDead", 0x173),
    ("AllowMotiveForceWhileAirborne", 0x174),
    ("Apply2DFrictionWhenAirborne", 0x175),
    ("DownhillOnly", 0x176),
    ("StickToGround", 0x177),
    ("CanMoveBackward", 0x178),
    ("CanReverseMove", 0x179),
    ("UpdateWaterWadingConditions", 0x17a),
    ("HasSuspension", 0x17b),
    ("UseTerrainSmoothing", 0x17c),
    ("BurniningDeathIsCavalry", 0x17d),
    ("ChargeAvailable", 0x17e),
    ("ChargeIgnoresCondition", 0x17f),
    ("EnableHighSpeedTurnFlags", 0x180),
    ("WaitForFormation", 0x181),
    ("BackingUpStopWhenTurning", 0x182),
    ("ScalesWalls", 0x183),
    ("TurnWhileMoving", 0x184),
    ("ClampOrientationToPathTangent", 0x185),
    ("ReorientIfTurnTooSharp", 0x186),
    ("BrakeBeforeReorienting", 0x187),
    ("EasingTakeOffAndLanding", 0x188),
];

struct Field {
    name: &'static str,
    offset: usize,
    default: Option<String>,
    kind: String,
}

struct Schema {
    fields: Vec<Field>,
    attributes: Vec<&'static str>,
    surfaces: Vec<String>,
    appearance: Vec<String>,
    behavior_z: Vec<String>,
    formation: Vec<String>,
    jet_options: Vec<String>,
    terrain: Vec<String>,
}

fn schema() -> &'static Schema {
    static SCHEMA: OnceLock<Schema> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let locomotor = Document::parse(LOCOMOTOR_SCHEMA).expect("bundled locomotor schema");
        let terrain = Document::parse(TERRAIN_SCHEMA).expect("bundled terrain schema");
        let declaration = named_type(&locomotor, "LocomotorTemplate");
        let fields: Vec<_> = FIELD_OFFSETS
            .iter()
            .map(|&(name, offset)| {
                let definition = declaration
                    .descendants()
                    .find(|node| {
                        node.tag_name().name() == "attribute"
                            && node.attribute("name") == Some(name)
                    })
                    .expect("bundled locomotor field");
                Field {
                    name,
                    offset,
                    default: definition.attribute("default").map(str::to_owned),
                    kind: definition
                        .attribute("type")
                        .expect("locomotor field type")
                        .to_owned(),
                }
            })
            .collect();
        let attributes = std::iter::once("id")
            .chain(fields.iter().map(|field| field.name))
            .collect();
        Schema {
            fields,
            attributes,
            surfaces: schema_values(&locomotor, "Surface"),
            appearance: schema_values(&locomotor, "Appearance"),
            behavior_z: schema_values(&locomotor, "LocoZ"),
            formation: schema_values(&locomotor, "LocoF"),
            jet_options: schema_values(&locomotor, "JetLocomotorDataOption"),
            terrain: schema_values(&terrain, "TerrainClassType"),
        }
    })
}

fn named_type<'a>(doc: &'a Document<'static>, name: &str) -> Node<'a, 'static> {
    doc.root_element()
        .children()
        .find(|node| node.attribute("name") == Some(name))
        .expect("bundled schema type")
}

fn schema_values(doc: &Document<'static>, name: &str) -> Vec<String> {
    named_type(doc, name)
        .descendants()
        .filter(|node| node.tag_name().name() == "enumeration")
        .filter_map(|node| node.attribute("value").map(str::to_owned))
        .collect()
}

pub(super) fn locomotor_template(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let schema = schema();
    validate_attributes(node, &schema.attributes, path)?;
    let mut data = BinaryData {
        payload: vec![0; ROOT_SIZE],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for field in &schema.fields {
        let name = field.name;
        let offset = field.offset;
        let Some(raw) = node.attribute(name).or(field.default.as_deref()) else {
            continue;
        };
        let value = match field.kind.as_str() {
            "SageBool" => {
                data.payload[offset] =
                    boolean(node, name, field.default.as_deref() == Some("true"), path)?;
                continue;
            }
            "SageReal" => finite_number(raw, name, path)?.to_bits(),
            "SageInt" => sage_int(raw, name, path)?,
            "Percentage" => percentage(raw, name, path)?.to_bits(),
            "Angle" => sage_angle(raw, name, path)?.to_bits(),
            "Time" => sage_time(raw, name, path)?.to_bits(),
            "LocomotorSurfaceBitFlags" => flags(raw, &schema.surfaces, name, path)?,
            "Appearance" => enum_index(raw, &schema.appearance, name, path)?,
            "LocoZ" => enum_index(raw, &schema.behavior_z, name, path)?,
            "LocoF" => enum_index(raw, &schema.formation, name, path)?,
            "ModelConditionBitFlags" => {
                let start = data.allocate(offset, MODEL_CONDITION_BYTES)?;
                write_model_conditions(raw, start, &mut data)?;
                continue;
            }
            "ObjectStatusBitFlags" => {
                let start = data.allocate(offset, OBJECT_STATUS_BYTES)?;
                write_status_flags(raw, start, &mut data)?;
                continue;
            }
            "FXListRef" => {
                if !raw.trim().is_empty() {
                    write_import(raw, "FXList", offset, &mut data, &mut result)?;
                }
                continue;
            }
            kind => unreachable!("unsupported field type {kind} in bundled locomotor schema"),
        };
        data.word(offset, value);
    }
    let mut children = significant_children(node).peekable();
    if let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "BounceKickTerrainMap")))
    {
        write_bounce_reference(child, &mut data, &mut result)?;
    }
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "JetLocomotorData")))
    {
        write_jet(child, &mut data, schema)?;
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in LocomotorTemplate"));
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = finish_relocations(data.relocations);
    Ok(result)
}

fn write_bounce_reference(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    validate_attributes(node, &[], data.path)?;
    let start = data.allocate(BOUNCE_POINTER, 4)?;
    if has_content(node) {
        write_import(
            text_value(node, data.path)?,
            BOUNCE_MAPPING,
            start,
            data,
            result,
        )?;
    }
    Ok(())
}

fn write_jet(node: Node<'_, '_>, data: &mut BinaryData<'_>, schema: &Schema) -> io::Result<()> {
    const ATTRIBUTES: &[&str] = &[
        "Options",
        "AttackPathStartRunDistance",
        "AttackPathClimbDistance",
        "AttackPathDiveDistanceStart",
        "AttackPathDiveDistanceEnd",
    ];
    const OPTIONS: &str = "Options";
    validate_attributes(node, ATTRIBUTES, data.path)?;
    if has_content(node) {
        return Err(invalid(data.path, "unexpected content in JetLocomotorData"));
    }
    let start = data.allocate(JET_POINTER, 0x14)?;
    let options = flags(
        node.attribute(OPTIONS).unwrap_or(""),
        &schema.jet_options,
        OPTIONS,
        data.path,
    )?;
    data.word(start, options);
    for (index, &name) in ATTRIBUTES[1..].iter().enumerate() {
        if let Some(raw) = node.attribute(name) {
            data.word(
                start + (index + 1) * 4,
                finite_number(raw, name, data.path)?.to_bits(),
            );
        }
    }
    Ok(())
}

pub(super) fn bounce_kick_terrain_mapping(
    node: Node<'_, '_>,
    path: &Path,
) -> io::Result<Marshaled> {
    const TERRAIN_CLASS: &str = "TerrainClass";
    const MULTIPLIER: &str = "BounceMultiplier";
    validate_attributes(node, &["id"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 0xc],
        relocations: Vec::new(),
        path,
    };
    let terrain = &schema().terrain;
    let start = data.list(4, significant_children(node).count(), 8)?;
    for (index, mapping) in significant_children(node).enumerate() {
        if !mapping.has_tag_name((SAGE_NS, "Mapping")) || has_content(mapping) {
            return Err(invalid(
                path,
                "expected Mapping in BounceKickTerrainMapping",
            ));
        }
        validate_attributes(mapping, &[TERRAIN_CLASS, MULTIPLIER], path)?;
        let offset = start + index * 8;
        if let Some(raw) = mapping.attribute(TERRAIN_CLASS) {
            data.word(offset, enum_index(raw, terrain, TERRAIN_CLASS, path)?);
        }
        if let Some(raw) = mapping.attribute(MULTIPLIER) {
            data.word(offset + 4, percentage(raw, MULTIPLIER, path)?.to_bits());
        }
    }
    Ok(Marshaled {
        payload: data.payload,
        relocations: finish_relocations(data.relocations),
        ..Marshaled::default()
    })
}

fn sage_int(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    let value = value.trim();
    let number = if let Some(hex) = value.strip_prefix("0x") {
        i64::from_str_radix(hex, 16)
    } else {
        value.parse::<i64>()
    };
    number
        .map(|value| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32 as u32)
        .map_err(|_| invalid(path, format!("invalid {name} {value:?}")))
}

fn finish_relocations(mut relocations: Vec<u32>) -> Vec<u32> {
    if !relocations.is_empty() {
        relocations.push(u32::MAX);
    }
    relocations
}
