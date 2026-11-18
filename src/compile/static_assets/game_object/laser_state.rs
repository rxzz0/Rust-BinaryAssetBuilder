use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, sage_time, significant_children, text_value,
    validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::fields;
use super::secondary_damage::encode_status;
use super::spawn::id_list;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xe7b6_af0d;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const CONVERGING: u32 = 0x5f74_98f9;
    const SWEEPING: u32 = 0x3c93_4753;
    const STREAM: u32 = 0xfef8_5f95;
    const ANGLE: u32 = 0x4e16_ea4d;
    const REAL: u32 = 0x6458_64f3;
    const FX: u32 = 0x0d1c_8e83;
    const TIME: u32 = 0x9986_69e8;
    const WEAPON: u32 = 0xd046_dd22;
    const CONDITIONS: u32 = 0x67db_bfc9;
    const CONVERGING_FIELDS: &[(&str, u32, usize, &str)] = &[
        ("ConvergingAngle", ANGLE, 0x2c, "0d"),
        ("AngleRotation", ANGLE, 0x30, "0d"),
        ("Radius", REAL, 0x34, "10"),
        ("SweepFXList", FX, 0x38, ""),
        ("VeteranSweepFXList", FX, 0x3c, ""),
        ("SweepFXTimeout", TIME, 0x40, "0s"),
        ("SweepWeapon", WEAPON, 0x44, ""),
        ("ModelConditions", CONDITIONS, 0x48, ""),
        ("Lifetime", TIME, 0x84, "1s"),
    ];
    const SWEEPING_FIELDS: &[(&str, u32, usize, &str)] = &[
        ("Radius", REAL, 0x2c, "10"),
        ("SweepFXList", FX, 0x30, ""),
        ("VeteranSweepFXList", FX, 0x34, ""),
        ("SweepFXTimeout", TIME, 0x38, "0s"),
        ("SweepWeapon", WEAPON, 0x3c, ""),
    ];

    let path = data.path;
    if node.has_tag_name((SAGE_NS, "StreamState")) {
        validate_attributes(node, &["id", "StreamId"], path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected StreamState content"));
        }
        let start = data.allocate(pointer, 12)?;
        let id = state_id(node, "StreamId", path)?;
        let module = module_base(node, STREAM, start, data, true)?;
        data.word(start + 8, id);
        return Ok((
            STREAM,
            Table::record(vec![
                module,
                Entry::named("StreamId", 0x23e6_50b8, Target::Data(start + 8)),
            ]),
        ));
    }
    let (kind, size, derived) = match node.tag_name().name() {
        "ConvergingLaserState" => (CONVERGING, 0x88, CONVERGING_FIELDS),
        "SweepingLaserState" => (SWEEPING, 0x40, SWEEPING_FIELDS),
        _ => (KIND, 0x2c, &[][..]),
    };
    let allowed: Vec<_> = ["id", "LaserId", "OriginBoneName"]
        .into_iter()
        .chain(derived.iter().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    let mut entries = vec![module_base(node, kind, start, data, true)?];
    for attribute in node.attributes() {
        if let Some(&(_, field_type, offset, _)) = derived
            .iter()
            .find(|&&(name, kind, ..)| name == attribute.name() && matches!(kind, FX | WEAPON))
        {
            let asset_type = if field_type == FX {
                "FXList"
            } else {
                "WeaponTemplate"
            };
            let reference = super::passive_area::reference(
                attribute.value(),
                asset_type,
                path,
                references,
                reference_types,
            )?;
            data.word(start + offset, reference);
        }
    }
    let mut derived_tokens = Vec::with_capacity(derived.len());
    for &(name, field_type, offset, default) in derived {
        let value = node.attribute(name).unwrap_or(default);
        let field = start + offset;
        match field_type {
            ANGLE => data.word(
                field,
                super::slow_death::angle(value, name, data)?.to_bits(),
            ),
            REAL => data.word(field, finite_number(value, name, path)?.to_bits()),
            TIME => data.word(field, sage_time(value, name, path)?.to_bits()),
            CONDITIONS => super::draws::write_model_conditions(value, field, data)?,
            FX | WEAPON => {}
            _ => unreachable!(),
        }
        derived_tokens.push(Entry::named(name, field_type, Target::Data(field)));
    }
    data.word(start + 8, state_id(node, "LaserId", path)?);
    if let Some(bone) = node.attribute("OriginBoneName") {
        data.string(start + 0xc, bone)?;
    }
    entries.extend(fields(
        start,
        &[
            ("LaserId", 0x23e6_50b8, 8),
            ("OriginBoneName", 0x1a29_33aa, 0xc),
        ],
    ));
    let mut children = significant_children(node).peekable();
    for (name, offset) in [
        ("LaserEndParticleSystem", 0x14),
        ("LaserStartParticleSystem", 0x1c),
    ] {
        let mut ids = Vec::new();
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            let value = text_value(child, path)?;
            let plain = value.split_once('\\').map_or(value, |(name, _)| name);
            let id = asset_reference(plain, "FXParticleSystemTemplate", path)?;
            references.push(id.into());
            reference_types.push(id.type_id);
            ids.push(references.len() as u32);
        }
        let table = id_list(data, start + offset, ids)?;
        entries.push(Entry::named(name, 0x43f1_ce36, Target::Table(table)));
    }
    let end =
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "EndOffset"))) {
            let axes = ["x", "y", "z"];
            validate_attributes(child, &axes, path)?;
            if has_content(child) {
                return Err(invalid(path, "unexpected EndOffset content"));
            }
            let offset = data.allocate(start + 0x24, 12)?;
            for (index, name) in axes.into_iter().enumerate() {
                let value = finite_number(child.attribute(name).unwrap_or("0"), name, path)?;
                data.word(offset + index * 4, value.to_bits());
            }
            Target::Data(offset)
        } else {
            Target::Null
        };
    let status =
        match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectStatusValidation"))) {
            Some(child) => Target::Table(encode_status(child, start + 0x28, data)?),
            None => Target::Null,
        };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected LaserState child"));
    }
    entries.extend([
        Entry::named("EndOffset", 0x2936_8077, end),
        Entry::named("ObjectStatusValidation", 0x3e8f_938c, status),
    ]);
    let table = Table::record(entries);
    if kind == KIND {
        return Ok((kind, table));
    }
    derived_tokens.insert(0, base(KIND, table));
    Ok((kind, Table::record(derived_tokens)))
}

fn state_id(node: Node<'_, '_>, name: &str, path: &std::path::Path) -> io::Result<u32> {
    node.attribute(name)
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map(|value| value as u32)
        .map_err(|_| invalid(path, format!("invalid {name}")))
}
