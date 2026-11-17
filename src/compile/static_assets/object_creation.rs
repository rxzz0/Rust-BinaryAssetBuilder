use super::binary::BinaryData;
use super::game_object::{model_condition_index, weapon_slot, write_model_conditions};
use super::{
    Marshaled, asset_reference, finite_number, percentage, sage_angle, sage_time, schema_enums,
    significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const ACTIONS: [(&str, usize); 6] = [
    ("CreateObject", 0xf8),
    ("Attack", 0x10),
    ("FireWeapon", 0x08),
    ("FlingObject", 0x34),
    ("StrafeAreaSquad", 0x20),
    ("ParabolicCurve", 0x30),
];

const SCHEMA: &str = include_str!("../../../schemas/ra3/xsd/AssetTypeObjectCreationList.xsd");

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    static FLAGS: OnceLock<[Vec<String>; 4]> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        schema_enums(
            SCHEMA,
            [
                "OCLType",
                "OCNuggetOption",
                "DispositionType",
                "ParabolicOption",
            ],
        )
    });
    validate_attributes(node, &["id", "Flags"], path)?;
    let [create, attack, fire, fling, strafe, curve] =
        collect_children(node, &ACTIONS.map(|(name, _)| name), path)?;
    let mut data = BinaryData {
        path,
        payload: vec![0; 0x38],
        relocations: Vec::new(),
    };
    let mut result = Marshaled::default();
    data.word(
        4,
        bit_flags(node.attribute("Flags").unwrap_or("NONE"), &flags[0], path)? as u32,
    );
    for (index, nodes) in [create, attack, fire, fling, strafe, curve]
        .into_iter()
        .enumerate()
    {
        let (name, size) = ACTIONS[index];
        let start = data.list(8 + index * 8, nodes.len(), size)?;
        for (index, child) in nodes.into_iter().enumerate() {
            let start = start + index * size;
            match name {
                "CreateObject" => create_object(child, start, &mut data, &mut result, flags)?,
                "Attack" => attack_action(child, start, &mut data)?,
                "FireWeapon" => {
                    validate_attributes(child, &["Weapon"], path)?;
                    expect_empty(child, path)?;
                    optional_import(
                        child,
                        "Weapon",
                        "WeaponTemplate",
                        start + 4,
                        &mut data,
                        &mut result,
                    )?;
                }
                "FlingObject" => fling_object(child, start, &mut data, &mut result)?,
                "StrafeAreaSquad" => strafe_area(child, start, &mut data, &mut result)?,
                "ParabolicCurve" => {
                    parabolic_curve(child, start, &mut data, &mut result, &flags[3])?
                }
                _ => unreachable!(),
            }
        }
    }
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.sort_unstable();
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

#[derive(Clone, Copy)]
enum Scalar {
    Real,
    Angle,
    Time,
    Percentage,
    Velocity,
}

type Field = (&'static str, usize, &'static str, Scalar);

const CREATE_FIELDS: &[Field] = &[
    ("VelocityScale", 0x1c, "1", Scalar::Real),
    ("MinForceMagnitude", 0x20, "0", Scalar::Real),
    ("MaxForceMagnitude", 0x24, "0", Scalar::Real),
    ("MinLifetime", 0x28, "0s", Scalar::Time),
    ("MaxLifetime", 0x2c, "0s", Scalar::Time),
    ("MinDistanceAFormation", 0x30, "0", Scalar::Real),
    ("MinDistanceBFormation", 0x34, "0", Scalar::Real),
    ("MaxDistanceFormation", 0x38, "0", Scalar::Real),
    ("FadeTime", 0x3c, "0s", Scalar::Time),
    ("TempModelConditionTime", 0x80, "0s", Scalar::Time),
    ("DispositionIntensity", 0x84, "0", Scalar::Velocity),
    ("DispositionAngle", 0x94, "0r", Scalar::Angle),
    ("MinForcePitch", 0x98, "0r", Scalar::Angle),
    ("MaxForcePitch", 0x9c, "0r", Scalar::Angle),
    ("MinForceYaw", 0xa0, "0r", Scalar::Angle),
    ("MaxForceYaw", 0xa4, "359d", Scalar::Angle),
    ("OrientationOffset", 0xa8, "0r", Scalar::Angle),
    ("MinHordeDensity", 0xb0, "100", Scalar::Percentage),
    ("MaxHordeDensity", 0xb4, "100", Scalar::Percentage),
    ("MinHealth", 0xb8, "1", Scalar::Real),
    ("MaxHealth", 0xbc, "1", Scalar::Real),
    ("InvulnerableTime", 0xc0, "0s", Scalar::Time),
    ("StartingBusyTime", 0xc4, "0s", Scalar::Time),
    ("JustBuiltFrames", 0xc8, "0s", Scalar::Time),
];

fn create_object(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
    flags: &[Vec<String>; 4],
) -> io::Result<()> {
    const OTHER_FIELDS: &[&str] = &[
        "PutInContainer",
        "CreateFX",
        "Count",
        "Options",
        "Disposition",
        "StartingModelConditions",
        "TempModelCondition",
        "DestinationPlayer",
        "WaypointSpawn",
        "FadeSound",
        "VeterancyLevel",
        "UseUpgradedLocomotor",
        "DisabledWhileBusy",
        "FireSpecialPowerTemplate",
        "StatusToCopyFromParent",
    ];
    let allowed: Vec<_> = OTHER_FIELDS
        .iter()
        .copied()
        .chain(CREATE_FIELDS.iter().map(|&(name, _, _, _)| name))
        .collect();
    let path = data.path;
    validate_attributes(node, &allowed, path)?;
    let [offset, required_upgrades, forbidden_upgrades, objects] = collect_children(
        node,
        &[
            "Offset",
            "RequiredUpgrade",
            "ForbiddenUpgrade",
            "CreateObject",
        ],
        path,
    )?;

    if let Some(value) = node.attribute("PutInContainer") {
        weak_object(value, start + 4, data, result)?;
    }
    optional_import(node, "CreateFX", "FXList", start + 8, data, result)?;
    data.word(start + 0xc, unsigned(node, "Count", 1, path)?);
    let options = bit_flags(node.attribute("Options").unwrap_or(""), &flags[1], path)?;
    data.word(start + 0x10, options as u32);
    data.word(start + 0x14, (options >> 32) as u32);
    data.word(
        start + 0x18,
        bit_flags(node.attribute("Disposition").unwrap_or(""), &flags[2], path)? as u32,
    );
    write_fields(node, start, CREATE_FIELDS, data)?;
    if let Some(value) = node.attribute("StartingModelConditions") {
        for flag in value
            .split_ascii_whitespace()
            .filter(|&flag| flag != "INVALID")
        {
            write_model_conditions(flag, start + 0x40, data)?;
        }
    }
    let condition = match node
        .attribute("TempModelCondition")
        .unwrap_or("INVALID")
        .trim()
    {
        "INVALID" => u32::MAX,
        value => model_condition_index(value, path)? as u32,
    };
    data.word(start + 0x7c, condition);
    if let Some(value) = node.attribute("DestinationPlayer") {
        let string = data.allocate(start + 0x88, 8)?;
        data.string(string, value)?;
    }
    if let Some(value) = node.attribute("WaypointSpawn") {
        data.string(start + 0x8c, value)?;
    }
    optional_import(
        node,
        "FadeSound",
        "BaseAudioEventInfo",
        start + 0xac,
        data,
        result,
    )?;
    let veterancy = node
        .attribute("VeterancyLevel")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid VeterancyLevel"))?;
    data.word(start + 0xcc, veterancy as u32);
    optional_import(
        node,
        "FireSpecialPowerTemplate",
        "SpecialPowerTemplate",
        start + 0xd0,
        data,
        result,
    )?;
    if let Some(value) = node.attribute("StatusToCopyFromParent") {
        static STATUS: OnceLock<[Vec<String>; 1]> = OnceLock::new();
        let [status] = STATUS.get_or_init(|| {
            schema_enums(
                include_str!("../../../schemas/ra3/xsd/Includes/ObjectStatus.xsd"),
                ["ObjectStatusType"],
            )
        });
        data.optional_word(start + 0xd4, enumeration(value, status, path)? as u32)?;
    }
    data.payload[start + 0xf4] = boolean(node, "UseUpgradedLocomotor", false, path)?;
    data.payload[start + 0xf5] = boolean(node, "DisabledWhileBusy", false, path)?;

    vector(&offset, start + 0xd8, data)?;
    for (nodes, offset) in [(required_upgrades, 0xdc), (forbidden_upgrades, 0xe4)] {
        let list = data.list(start + offset, nodes.len(), 4)?;
        for (index, child) in nodes.into_iter().enumerate() {
            write_import(
                text_value(child, path)?,
                "UpgradeTemplate",
                list + index * 4,
                data,
                result,
            )?;
        }
    }
    let list = data.list(start + 0xec, objects.len(), 4)?;
    for (index, child) in objects.into_iter().enumerate() {
        weak_object(text_value(child, path)?, list + index * 4, data, result)?;
    }
    Ok(())
}

fn attack_action(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let path = data.path;
    validate_attributes(
        node,
        &["NumberOfShots", "WeaponSlot", "DeliveryDecalRadius"],
        path,
    )?;
    expect_empty(node, path)?;
    data.word(start + 4, unsigned(node, "NumberOfShots", 1, path)?);
    data.word(
        start + 8,
        weapon_slot(
            node.attribute("WeaponSlot").unwrap_or("PRIMARY_WEAPON"),
            path,
        )?,
    );
    write_fields(
        node,
        start,
        &[("DeliveryDecalRadius", 0xc, "0", Scalar::Real)],
        data,
    )
}

fn fling_object(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "CreateObject",
            "Speed",
            "Height",
            "UsePrimaryObjectForOffsets",
            "Count",
            "InitialDelay",
        ],
        path,
    )?;
    let [offset, distance, angle, target] = collect_children(
        node,
        &[
            "Offset",
            "DistanceFromTarget",
            "AngleAroundTarget",
            "DistanceAtTarget",
        ],
        path,
    )?;
    weak_object(
        required(node, "CreateObject", path)?,
        start + 4,
        data,
        result,
    )?;
    required(node, "Speed", path)?;
    write_fields(
        node,
        start,
        &[
            ("Speed", 8, "0", Scalar::Real),
            ("Height", 0xc, "0", Scalar::Real),
            ("InitialDelay", 0x14, "0s", Scalar::Time),
        ],
        data,
    )?;
    data.word(start + 0x10, unsigned(node, "Count", 1, path)?);
    data.payload[start + 0x30] = boolean(node, "UsePrimaryObjectForOffsets", false, path)?;
    vector(&offset, start + 0x18, data)?;
    for (nodes, name, offset) in [
        (distance, "DistanceFromTarget", 0x1c),
        (angle, "AngleAroundTarget", 0x24),
    ] {
        let child =
            single(&nodes, path)?.ok_or_else(|| invalid(path, format!("expected {name}")))?;
        real_range(child, start + offset, data)?;
    }
    if let Some(child) = single(&target, path)? {
        let range = data.allocate(start + 0x2c, 8)?;
        real_range(child, range, data)?;
    }
    Ok(())
}

fn strafe_area(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "CreateObject",
            "Count",
            "Height",
            "DistanceFromTarget",
            "MaintainOffestToTarget",
        ],
        path,
    )?;
    let [inter, intra] = collect_children(node, &["InterOffset", "IntraOffset"], path)?;
    weak_object(
        required(node, "CreateObject", path)?,
        start + 4,
        data,
        result,
    )?;
    data.word(start + 8, unsigned(node, "Count", 1, path)?);
    write_fields(
        node,
        start,
        &[
            ("Height", 0xc, "0", Scalar::Real),
            ("DistanceFromTarget", 0x10, "0", Scalar::Real),
        ],
        data,
    )?;
    data.payload[start + 0x1c] = boolean(node, "MaintainOffestToTarget", true, path)?;
    vector(&inter, start + 0x14, data)?;
    vector(&intra, start + 0x18, data)
}

fn parabolic_curve(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
    flags: &[String],
) -> io::Result<()> {
    const FIELDS: &[Field] = &[
        ("InitialDelay", 0xc, "0s", Scalar::Time),
        ("ApproachTime", 0x10, "0s", Scalar::Time),
        ("StartDistanceFromTarget", 0x14, "0", Scalar::Real),
        ("EndDistanceFromTarget", 0x18, "0", Scalar::Real),
        ("CruiseHeight", 0x1c, "0", Scalar::Real),
        ("AttackHeight", 0x20, "0", Scalar::Real),
        ("ApproachRoll", 0x24, "0r", Scalar::Angle),
        ("LeaveRoll", 0x28, "0r", Scalar::Angle),
    ];
    let path = data.path;
    let allowed: Vec<_> = ["Options", "CreateObject"]
        .into_iter()
        .chain(FIELDS.iter().map(|&(name, _, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let [offset] = collect_children(node, &["Offset"], path)?;
    data.word(
        start + 4,
        bit_flags(node.attribute("Options").unwrap_or(""), flags, path)? as u32,
    );
    weak_object(
        required(node, "CreateObject", path)?,
        start + 8,
        data,
        result,
    )?;
    write_fields(node, start, FIELDS, data)?;
    vector(&offset, start + 0x2c, data)
}

fn collect_children<'a, 'input, const N: usize>(
    node: Node<'a, 'input>,
    names: &[&str; N],
    path: &Path,
) -> io::Result<[Vec<Node<'a, 'input>>; N]> {
    let mut groups = std::array::from_fn(|_| Vec::new());
    for child in significant_children(node) {
        let index = names
            .iter()
            .position(|name| child.has_tag_name((SAGE_NS, *name)))
            .ok_or_else(|| {
                invalid(
                    path,
                    format!("unexpected {} content", node.tag_name().name()),
                )
            })?;
        groups[index].push(child);
    }
    Ok(groups)
}

fn single<'a, 'input>(
    nodes: &[Node<'a, 'input>],
    path: &Path,
) -> io::Result<Option<Node<'a, 'input>>> {
    match nodes {
        [] => Ok(None),
        [child] => Ok(Some(*child)),
        [first, ..] => Err(invalid(
            path,
            format!("duplicate {}", first.tag_name().name()),
        )),
    }
}

fn vector(nodes: &[Node<'_, '_>], pointer: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    if let Some(node) = single(nodes, data.path)? {
        validate_attributes(node, &["x", "y", "z"], data.path)?;
        expect_empty(node, data.path)?;
        let start = data.allocate(pointer, 12)?;
        write_fields(
            node,
            start,
            &[
                ("x", 0, "0", Scalar::Real),
                ("y", 4, "0", Scalar::Real),
                ("z", 8, "0", Scalar::Real),
            ],
            data,
        )?;
    }
    Ok(())
}

fn real_range(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    validate_attributes(node, &["Low", "High"], data.path)?;
    expect_empty(node, data.path)?;
    for (index, name) in ["Low", "High"].into_iter().enumerate() {
        let value = finite_number(required(node, name, data.path)?, name, data.path)?;
        data.word(start + index * 4, value.to_bits());
    }
    Ok(())
}

fn expect_empty(node: Node<'_, '_>, path: &Path) -> io::Result<()> {
    if has_content(node) {
        return Err(invalid(
            path,
            format!("unexpected {} content", node.tag_name().name()),
        ));
    }
    Ok(())
}

fn write_fields(
    node: Node<'_, '_>,
    start: usize,
    fields: &[Field],
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for &(name, offset, default, kind) in fields {
        let raw = node.attribute(name).unwrap_or(default);
        let value = match kind {
            Scalar::Real => finite_number(raw, name, data.path)?,
            Scalar::Angle => sage_angle(raw, name, data.path)?,
            Scalar::Time => sage_time(raw, name, data.path)?,
            Scalar::Percentage => percentage(raw, name, data.path)?,
            Scalar::Velocity => finite_number(raw, name, data.path)? * 0.2,
        };
        data.word(start + offset, value.to_bits());
    }
    Ok(())
}

fn optional_import(
    node: Node<'_, '_>,
    name: &str,
    target: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    if let Some(value) = node
        .attribute(name)
        .filter(|value| !value.trim().is_empty())
    {
        write_import(value, target, offset, data, result)?;
    }
    Ok(())
}

fn weak_object(
    value: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let asset = asset_reference(value, "GameObject", data.path)?;
    result.reference_types.push(asset.type_id);
    data.word(offset, asset.instance_id);
    Ok(())
}

fn unsigned(node: Node<'_, '_>, field: &str, default: u32, path: &Path) -> io::Result<u32> {
    node.attribute(field).map_or(Ok(default), |value| {
        value
            .trim()
            .parse()
            .map_err(|_| invalid(path, format!("invalid {field}")))
    })
}

fn boolean(node: Node<'_, '_>, field: &str, default: bool, path: &Path) -> io::Result<u8> {
    match node.attribute(field).map(str::trim) {
        None => Ok(u8::from(default)),
        Some("true" | "1") => Ok(1),
        Some("false" | "0") => Ok(0),
        _ => Err(invalid(path, format!("invalid {field}"))),
    }
}

fn enumeration(value: &str, names: &[String], path: &Path) -> io::Result<usize> {
    names
        .iter()
        .position(|name| name == value.trim())
        .ok_or_else(|| {
            invalid(
                path,
                format!("invalid ObjectCreationList enum value {value}"),
            )
        })
}

fn bit_flags(value: &str, names: &[String], path: &Path) -> io::Result<u64> {
    let mut bits = 0;
    for value in value.split_ascii_whitespace() {
        bits |= 1u64 << enumeration(value, names, path)?;
    }
    Ok(bits)
}
