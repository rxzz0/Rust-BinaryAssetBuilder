use super::super::binary::BinaryData;
use super::super::{
    Marshaled, finite_number, required_element, significant_children, validate_attributes,
};
use super::{boolean, random_variable, vector};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

#[derive(Clone, Copy)]
enum Field {
    Random,
    Vector,
    RandomVector,
}

use Field::{Random, RandomVector, Vector};

pub(super) fn physics(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    _out: &mut Marshaled,
) -> io::Result<()> {
    match node.tag_name().name() {
        "Default" => {
            validate_attributes(
                node,
                &["Gravity", "Swirly", "ParticlesAttachToBone"],
                data.path,
            )?;
            let start = record(pointer, 36, 0xb966_43ff, data)?;
            real(node, "Gravity", start + 4, data)?;
            data.payload[start + 32] = boolean(node, "Swirly", false, data.path)?;
            data.payload[start + 33] = boolean(node, "ParticlesAttachToBone", false, data.path)?;
            fields(
                node,
                start + 8,
                &[("VelocityDamping", Random), ("DriftVelocity", Vector)],
                0,
                data,
            )
        }
        "Swarm" => {
            validate_attributes(node, &["AttractStrength"], data.path)?;
            let start = record(pointer, 8, 0x75cd_18b3, data)?;
            real(node, "AttractStrength", start + 4, data)?;
            fields(node, start, &[], 0, data)
        }
        tag => Err(invalid(
            data.path,
            format!("unsupported particle physics {tag}"),
        )),
    }
}

pub(super) fn volume(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    _out: &mut Marshaled,
) -> io::Result<()> {
    const ENDPOINTS: &[(&str, Field)] = &[("StartPoint", Vector), ("EndPoint", Vector)];
    let (kind, size, attributes, children): (_, _, &[_], &[_]) = match node.tag_name().name() {
        "Box" => (0x4a0b_96f4, 20, &["IsHollow"], &[("HalfSize", Vector)]),
        "Cylinder" => (
            0x5c8a_6f16,
            32,
            &["IsHollow", "Radius", "RadiusRate", "Length"],
            &[("Offset", Vector)],
        ),
        "Line" => (0x0da3_5f9c, 32, &["IsHollow"], ENDPOINTS),
        "Spline" => (0x2f6f_162a, 32, &["IsHollow"], ENDPOINTS),
        "Point" => (0xd32f_c96b, 8, &["IsHollow"], &[]),
        "Sphere" => (0xedf8_3b69, 12, &["IsHollow", "Radius"], &[]),
        "TerrainFire" => (
            0xe0e0_3e35,
            48,
            &["IsHollow", "CellEmissionChance"],
            &[("Offset", RandomVector)],
        ),
        "Lightning" => (0x54d7_12bf, 8, &["IsHollow"], &[]),
        tag => {
            return Err(invalid(
                data.path,
                format!("unsupported particle volume {tag}"),
            ));
        }
    };
    validate_attributes(node, attributes, data.path)?;
    let start = record(pointer, size, kind, data)?;
    data.payload[start + 4] = boolean(node, "IsHollow", false, data.path)?;
    for (index, name) in attributes[1..].iter().enumerate() {
        real(node, name, start + 8 + index * 4, data)?;
    }
    fields(
        node,
        start + 4 + attributes.len() * 4,
        children,
        children.len(),
        data,
    )
}

pub(super) fn velocity(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    _out: &mut Marshaled,
) -> io::Result<()> {
    let (kind, size, children): (_, _, &[_]) = match node.tag_name().name() {
        "Cylindrical" => (0xf1ec_3b2e, 28, &[("Radial", Random), ("Normal", Random)]),
        "Ortho" => (0xe752_8102, 40, &[("Position", RandomVector)]),
        "Outward" => (
            0xcf2f_d6df,
            28,
            &[("Speed", Random), ("OtherSpeed", Random)],
        ),
        "Spherical" => (0x8bb3_8534, 16, &[("Speed", Random)]),
        "Wavefront" => {
            validate_attributes(node, &["Shape"], data.path)?;
            let start = record(pointer, 68, 0xecf7_9bc7, data)?;
            let shape = match node.attribute("Shape").unwrap_or("CIRCLE").trim() {
                "INVALID" => 0,
                "CIRCLE" => 1,
                "LINE" => 2,
                value => {
                    return Err(invalid(
                        data.path,
                        format!("unsupported Wavefront Shape {value:?}"),
                    ));
                }
            };
            data.word(start + 4, shape);
            return fields(
                node,
                start + 8,
                &[
                    ("Velocity", Random),
                    ("Amplitude", Random),
                    ("InitialRadius", Random),
                    ("LineWidth", Random),
                    ("LineAngle", Random),
                ],
                3,
                data,
            );
        }
        tag => {
            return Err(invalid(
                data.path,
                format!("unsupported particle velocity {tag}"),
            ));
        }
    };
    validate_attributes(node, &[], data.path)?;
    let start = record(pointer, size, kind, data)?;
    fields(node, start + 4, children, children.len(), data)
}

fn record(pointer: usize, size: usize, kind: u32, data: &mut BinaryData<'_>) -> io::Result<usize> {
    let start = data.allocate(pointer, size)?;
    data.word(start, kind);
    Ok(start)
}

fn real(node: Node<'_, '_>, name: &str, start: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let value = finite_number(node.attribute(name).unwrap_or("0"), name, data.path)?;
    data.word(start, value.to_bits());
    Ok(())
}

fn fields(
    node: Node<'_, '_>,
    mut start: usize,
    layout: &[(&str, Field)],
    required: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let mut children = significant_children(node).peekable();
    for (index, &(name, field)) in layout.iter().enumerate() {
        if index < required
            || children
                .peek()
                .is_some_and(|child| child.has_tag_name((SAGE_NS, name)))
        {
            let child = required_element(&mut children, name, data.path)?;
            match field {
                Random => random_variable(child, start, data)?,
                Vector => vector(child, start, data)?,
                RandomVector => {
                    validate_attributes(child, &[], data.path)?;
                    fields(
                        child,
                        start,
                        &[("x", Random), ("y", Random), ("z", Random)],
                        3,
                        data,
                    )?;
                }
            }
        }
        start += match field {
            Random | Vector => 12,
            RandomVector => 36,
        };
    }
    if children.next().is_some() {
        return Err(invalid(
            data.path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    Ok(())
}
