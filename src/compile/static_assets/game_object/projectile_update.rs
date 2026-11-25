use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, percentage, sage_angle, significant_children, text_value,
    validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::fields;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x114c_c62f;
const REAL: u32 = 0x6458_64f3;
const ANGLE: u32 = 0x4e16_ea4d;
const FIELDS: [(&str, u32, usize); 4] = [
    ("StateMachine", 0x0e8a_0a05, 8),
    ("MinDetonationHeight", REAL, 0xc),
    ("MinTargetDistance", REAL, 0x10),
    ("Flags", 0xd0ff_3633, 0x14),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "StateMachine",
            "MinDetonationHeight",
            "MinTargetDistance",
            "Flags",
        ],
        path,
    )?;
    let mut children = significant_children(node);
    let movement = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "Movement")))
        .ok_or_else(|| invalid(path, "ProjectileUpdate requires Movement"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ProjectileUpdate child"));
    }
    validate_attributes(movement, &[], path)?;
    let mut children = significant_children(movement);
    let variant = children
        .next()
        .filter(|child| child.tag_name().namespace() == Some(SAGE_NS))
        .ok_or_else(|| invalid(path, "Movement requires a projectile variant"))?;
    if children.next().is_some() {
        return Err(invalid(
            path,
            "Movement accepts only one projectile variant",
        ));
    }

    let start = data.allocate(pointer, 0x1c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let state_machine = enum_value(
        node.attribute("StateMachine").unwrap_or("BombProjectile"),
        &[
            "BombProjectile",
            "StraightLineProjectile",
            "BalloonProjectile",
            "BezierSplineProjectile",
        ],
        path,
    )?;
    data.word(start + 8, state_machine);
    for (name, offset) in [("MinDetonationHeight", 0xc), ("MinTargetDistance", 0x10)] {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let mut flags = 0;
    for flag in node
        .attribute("Flags")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        if flag != "LAUNCH_AT_TARGET_POS" {
            return Err(invalid(
                path,
                format!("unknown ProjectileUpdate flag {flag}"),
            ));
        }
        flags |= 1;
    }
    data.word(start + 0x14, flags);
    entries.extend(fields(start, &FIELDS));

    entries.push(Entry::named(
        "Movement",
        0x9f29_b19e,
        Target::Table(encode_movement(
            variant,
            start + 0x18,
            data,
            references,
            reference_types,
        )?),
    ));
    Ok((KIND, Table::record(entries)))
}

fn encode_movement(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const BOMB: &[(&str, u32, usize, &str)] = &[
        ("InitialSpeed", REAL, 4, "0"),
        ("Acceleration", REAL, 8, "0"),
        ("TerminalVelocity", REAL, 0xc, "0"),
        ("RotationSpeedPerSecond", ANGLE, 0x10, "0d"),
        ("TerminalRotation", ANGLE, 0x14, "90d"),
        ("ForwardVelocityDamping", REAL, 0x18, "100%"),
    ];
    const STRAIGHT: &[(&str, u32, usize, &str)] = &[
        ("MuzzleVelocity", REAL, 4, "0"),
        ("Acceleration", REAL, 8, "0"),
        ("TerminalVelocity", REAL, 0xc, "0"),
    ];
    const BEZIER: &[(&str, u32, usize, &str)] = &[
        ("Velocity", REAL, 4, ""),
        ("Range", REAL, 8, ""),
        ("Gravity", REAL, 0xc, "1"),
    ];
    let path = data.path;
    let (kind, size, layout) = match node.tag_name().name() {
        "MoveToGround" => (0x8776_7df6, 0x1c, BOMB),
        "MoveInStraightLine" => (0x858c_bfdc, 0x10, STRAIGHT),
        "BalloonMoveToGround" => (0x1a5b_5a69, 4, &[][..]),
        "BezierSpline" => (0xb38f_4984, 0x18, BEZIER),
        tag => {
            return Err(invalid(
                path,
                format!("unsupported ProjectileUpdate movement {tag}"),
            ));
        }
    };
    validate_attributes(
        node,
        &layout.iter().map(|&(name, ..)| name).collect::<Vec<_>>(),
        path,
    )?;

    let start = data.allocate(pointer, size)?;
    data.word(start, kind);
    let mut entries = vec![base(
        0xabd1_b08a,
        Table::record(vec![Entry::new(
            0x19b0_42a0,
            0x23e6_50b8,
            Target::Data(start),
        )]),
    )];
    for &(name, token, offset, default) in layout {
        let value = if default.is_empty() {
            required(node, name, path)?
        } else {
            node.attribute(name).unwrap_or(default)
        };
        let value = match (name, token) {
            ("ForwardVelocityDamping", _) => percentage(value, name, path)?,
            (_, ANGLE) => sage_angle(value, name, path)?,
            _ => finite_number(value, name, path)?,
        };
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(name, token, Target::Data(start + offset)));
    }

    let mut children = significant_children(node);
    if node.has_tag_name((SAGE_NS, "BezierSpline")) {
        let paths = children.collect::<Vec<_>>();
        if paths.is_empty() {
            return Err(invalid(path, "BezierSpline requires at least one Path"));
        }
        let count = start + 0x10;
        let first = data.list(count, paths.len(), 4)?;
        let mut items = Vec::with_capacity(paths.len());
        for (index, child) in paths.into_iter().enumerate() {
            if !child.has_tag_name((SAGE_NS, "Path")) {
                return Err(invalid(path, "unexpected BezierSpline child"));
            }
            validate_attributes(child, &[], path)?;
            let reference = super::passive_area::reference(
                text_value(child, path)?,
                "ProjectilePath",
                path,
                references,
                reference_types,
            )?;
            let offset = first + index * 4;
            data.word(offset, reference);
            items.push((0, Target::Data(offset)));
        }
        entries.push(Entry::named(
            "Path",
            0x2ccb_a11c,
            Target::Table(Table::array(count, items)),
        ));
    } else if children.next().is_some() {
        return Err(invalid(
            path,
            "unexpected ProjectileUpdate movement content",
        ));
    }

    Ok(Table::sequence(vec![Entry::new(
        2,
        kind,
        Target::Table(Table::record(entries)),
    )]))
}
