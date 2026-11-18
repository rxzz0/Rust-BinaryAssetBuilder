use super::super::ai_strategic::enum_index;
use super::super::{Marshaled, finite_number, invalid, percentage, schema_enums, write_import};
use super::{BinaryData, boolean, random_variable, vector};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const PARTICLE_ENUMS: &str =
    include_str!("../../../../schemas/ra3/xsd/FXParticleSystem/FXParticleBase.xsd");

pub(super) fn draw(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let kind = node.tag_name().name();
    let (id, size) = match kind {
        "Default" => (0xbed7_8e9d, 4),
        "Gpu" => (0x85fe_07df, 60),
        "LightSource" => (0x51da_e8c4, 8),
        "Lightning" => (0xb503_bc4c, 36),
        "RenderObject" => (0xcb25_505b, 20),
        "Streak" => (0xe30b_2280, 4),
        "Swarm" => (0x8df6_b125, 24),
        "Trail" => (0x6f33_23a5, 24),
        _ => {
            return Err(invalid(
                data.path,
                format!("unsupported particle Draw module {kind}"),
            ));
        }
    };
    let start = data.allocate(pointer, size)?;
    data.word(start, id);
    match kind {
        "Gpu" => {
            data.string(
                start + 4,
                node.attribute("Shader").unwrap_or("GPUParticle.fx"),
            )?;
            for (i, (field, default)) in [
                ("FramesPerRow", 1),
                ("TotalFrames", 1),
                ("SingleRow", 0),
                ("RandTex", 0),
            ]
            .into_iter()
            .enumerate()
            {
                integer(node, field, default, start + 12 + i * 4, data)?;
            }
            reference(node, "DetailTexture", "Texture", start + 28, data, out)?;
            reals(node, start + 32, &[("SpeedMultiplier", 1.0)], data)?;
            enum_word(node, "GeometryType", "SIMPLE_QUAD", 0, start + 36, data)?;
            reals(
                node,
                start + 40,
                &[
                    ("MiscValue1", 0.0),
                    ("MiscValue2", 0.0),
                    ("MiscValue3", 0.0),
                    ("MiscValue4", 0.0),
                ],
                data,
            )?;
            data.payload[start + 56] = boolean(node, "SortParticles", false, data.path)?;
            data.payload[start + 57] = boolean(node, "EnableSoftParticles", true, data.path)?;
        }
        "LightSource" => reals(
            node,
            start + 4,
            &[("MaximumIntensityRadiusRatio", 0.0)],
            data,
        )?,
        "Lightning" => {
            vector(required(node, "StartPoint", data)?, start + 4, data)?;
            vector(required(node, "EndPoint", data)?, start + 16, data)?;
            let sets = fixed_list(node, "RandomSet", 2, data)?;
            let records = data.list(start + 28, sets.len(), 72)?;
            for (i, set) in sets.into_iter().enumerate() {
                randoms(
                    set,
                    records + i * 72,
                    &[
                        "StartAmplitude",
                        "EndAmplitudeChange",
                        "StartFrequency",
                        "EndFrequencyChange",
                        "StartPhase",
                        "EndPhaseChange",
                    ],
                    data,
                )?;
            }
        }
        "RenderObject" => {
            reals(node, start + 4, &[("SinkRate", 0.0)], data)?;
            let sets = fixed_list(node, "ObjectSet", 3, data)?;
            let records = data.list(start + 8, sets.len(), 16)?;
            for (i, set) in sets.into_iter().enumerate() {
                let record = records + i * 16;
                if let Some(group) = set.attribute("RenderGroup") {
                    data.string(record, group)?;
                }
                integer(set, "NumObjects", 0, record + 8, data)?;
                if let Some(value) = set.attribute("Percent") {
                    data.word(
                        record + 12,
                        percentage(value, "Percent", data.path)?.to_bits(),
                    );
                }
            }
            data.payload[start + 16] = boolean(node, "MultiRenderObjects", false, data.path)?;
            data.payload[start + 17] = boolean(node, "SinkOnTerrainCollision", false, data.path)?;
        }
        "Swarm" => {
            reals(
                node,
                start + 4,
                &[
                    ("OpaqueSpeed", 0.0),
                    ("TransparentSpeed", 100.0),
                    ("SpeedStretchAmount", 1.0),
                    ("AttractStrength", 0.0),
                ],
                data,
            )?;
            reference(node, "EnvironmentTexture", "Texture", start + 20, data, out)?;
        }
        "Trail" => {
            integer(node, "TrailLife", 30, start + 4, data)?;
            reals(
                node,
                start + 8,
                &[
                    ("UTile", 1.0),
                    ("VTile", 1.0),
                    ("UScrollRate", 0.0),
                    ("VScrollRate", 0.0),
                ],
                data,
            )?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    _out: &mut Marshaled,
) -> io::Result<()> {
    let kind = node.tag_name().name();
    let (id, size) = match kind {
        "Default" => (0xaede_a298, 104),
        "RenderObject" => (0x0dfb_6e4a, 200),
        _ => {
            return Err(invalid(
                data.path,
                format!("unsupported particle Update module {kind}"),
            ));
        }
    };
    let start = data.allocate(pointer, size)?;
    data.word(start, id);
    enum_word(node, "Rotation", "ROTATION_OFF", 1, start + 4, data)?;
    let values = start + 8;
    if kind == "Default" {
        for offset in [values + 12, values + 84] {
            data.word(offset, 1);
            data.word(offset + 4, 1.0_f32.to_bits());
            data.word(offset + 8, 1.0_f32.to_bits());
        }
        randoms(
            node,
            values,
            &[
                "SizeRate",
                "SizeRateDamping",
                "AngleZ",
                "AngularRateZ",
                "AngularDamping",
                "AngleXY",
                "AngularRateXY",
                "AngularDampingXY",
            ],
            data,
        )?;
    } else {
        randoms(
            node,
            values,
            &[
                "AngleX",
                "AngularRateX",
                "AngleY",
                "AngularRateY",
                "AngleZ",
                "AngularRateZ",
                "AngularDamping",
            ],
            data,
        )?;
        for (i, field) in ["StartSize", "SizeRate", "SizeDamping"]
            .into_iter()
            .enumerate()
        {
            let coord = required(node, field, data)?;
            for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
                random_variable(
                    required(coord, name, data)?,
                    values + 84 + i * 36 + axis * 12,
                    data,
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn event(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let kind = node.tag_name().name();
    let (id, size, field) = match kind {
        "Life" => (0xce96_a52d, 24, "EventTime"),
        "Collision" => (0x401e_3b7f, 28, "HeightOffset"),
        _ => {
            return Err(invalid(
                data.path,
                format!("unsupported particle Event module {kind}"),
            ));
        }
    };
    let start = data.allocate(pointer, size)?;
    data.word(start, id);
    reference(node, "EventFX", "FXList", start + 4, data, out)?;
    data.payload[start + 8] = boolean(node, "PerParticle", false, data.path)?;
    data.payload[start + 9] = boolean(node, "KillAfterEvent", false, data.path)?;
    random_variable(required(node, field, data)?, start + 12, data)?;
    if kind == "Collision" {
        data.payload[start + 24] = boolean(node, "OrientFXToTerrain", false, data.path)?;
    }
    Ok(())
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn required<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    data: &BinaryData<'_>,
) -> io::Result<Node<'a, 'input>> {
    child(node, name).ok_or_else(|| {
        invalid(
            data.path,
            format!("particle {} requires {name}", node.tag_name().name()),
        )
    })
}

fn fixed_list<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    count: usize,
    data: &BinaryData<'_>,
) -> io::Result<Vec<Node<'a, 'input>>> {
    let items: Vec<_> = node
        .children()
        .filter(|child| child.has_tag_name(name))
        .collect();
    if items.len() != count {
        return Err(invalid(
            data.path,
            format!(
                "particle {} requires {count} {name} elements",
                node.tag_name().name()
            ),
        ));
    }
    Ok(items)
}

fn randoms(
    node: Node<'_, '_>,
    offset: usize,
    names: &[&str],
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for (i, name) in names.iter().enumerate() {
        if let Some(value) = child(node, name) {
            random_variable(value, offset + i * 12, data)?;
        }
    }
    Ok(())
}

fn integer(
    node: Node<'_, '_>,
    field: &str,
    default: i32,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let value = match node.attribute(field).map(str::trim) {
        Some(value) => match value.strip_prefix("0x") {
            Some(hex) => u32::from_str_radix(hex, 16),
            None => value.parse::<i32>().map(|signed| signed as u32),
        }
        .map_err(|_| invalid(data.path, format!("invalid {field} {value}")))?,
        None => default as u32,
    };
    data.word(offset, value);
    Ok(())
}

fn reals(
    node: Node<'_, '_>,
    offset: usize,
    fields: &[(&str, f32)],
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for (i, &(field, default)) in fields.iter().enumerate() {
        let value = node
            .attribute(field)
            .map_or(Ok(default), |value| finite_number(value, field, data.path))?;
        data.word(offset + i * 4, value.to_bits());
    }
    Ok(())
}

fn enum_word(
    node: Node<'_, '_>,
    field: &str,
    default: &str,
    table: usize,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    static ENUMS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    let enums = ENUMS.get_or_init(|| {
        schema_enums(
            PARTICLE_ENUMS,
            [
                "FXParticleSystem_GeometryType",
                "FXParticleSystem_RotationType",
            ],
        )
    });
    let value = node.attribute(field).unwrap_or(default);
    data.word(offset, enum_index(value, &enums[table], field, data.path)?);
    Ok(())
}

fn reference(
    node: Node<'_, '_>,
    field: &str,
    kind: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    if let Some(value) = node
        .attribute(field)
        .filter(|value| !value.trim().is_empty())
    {
        write_import(value, kind, offset, data, out)?;
    }
    Ok(())
}
