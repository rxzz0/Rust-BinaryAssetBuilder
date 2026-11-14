use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, sync::OnceLock};

const KIND: u32 = 0x00dd_bb83;
const FIELDS: [(&str, u32, usize); 12] = [
    ("NodeLifetime", 0x9986_69e8, 8),
    ("DamageInterval", 0x9986_69e8, 0xc),
    ("Distance", 0x6458_64f3, 0x10),
    ("Damage", 0x6458_64f3, 0x14),
    ("DamageType", 0x4648_6ff1, 0x18),
    ("DeathType", 0xc608_ed36, 0x1c),
    ("DamageFXType", 0x2faa_4e62, 0x20),
    ("ObjectStatusForbiddenForDamage", 0xa7bd_811e, 0x24),
    ("MinDistanceTolerance", 0x6458_64f3, 0x40),
    ("MaxValidHeightOffGround", 0x6458_64f3, 0x44),
    ("FXParticleSystemTemplate", 0x43f1_ce36, 0x48),
    ("NodeFXList", 0x0d1c_8e83, 0x4c),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = ["id"]
        .into_iter()
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x58)?;
    let mut entries = vec![super::behaviors_basic::module_base(
        node, KIND, start, data, true,
    )?];
    for &(name, _, offset) in &FIELDS[..2] {
        data.word(
            start + offset,
            sage_time(required(node, name, path)?, name, path)?.to_bits(),
        );
    }
    for (name, value, offset) in [
        ("Distance", required(node, "Distance", path)?, 0x10),
        ("Damage", node.attribute("Damage").unwrap_or("0"), 0x14),
        (
            "MinDistanceTolerance",
            node.attribute("MinDistanceTolerance").unwrap_or("10"),
            0x40,
        ),
        (
            "MaxValidHeightOffGround",
            node.attribute("MaxValidHeightOffGround").unwrap_or("0"),
            0x44,
        ),
    ] {
        data.word(start + offset, finite_number(value, name, path)?.to_bits());
    }
    static DAMAGE: OnceLock<Vec<String>> = OnceLock::new();
    static DEATH: OnceLock<Vec<String>> = OnceLock::new();
    static EFFECT: OnceLock<Vec<String>> = OnceLock::new();
    for (name, default, offset, cache, schema) in [
        (
            "DamageType",
            "UNDEFINED",
            0x18,
            &DAMAGE,
            include_str!("../../../../schemas/ra3/xsd/Includes/DamageDef.xsd"),
        ),
        (
            "DeathType",
            "NORMAL",
            0x1c,
            &DEATH,
            include_str!("../../../../schemas/ra3/xsd/Includes/Death.xsd"),
        ),
        (
            "DamageFXType",
            "UNDEFINED",
            0x20,
            &EFFECT,
            include_str!("../../../../schemas/ra3/xsd/Includes/DamageFX.xsd"),
        ),
    ] {
        let value = node.attribute(name).unwrap_or(default);
        let options = cache.get_or_init(|| super::ai_update::enum_values(schema, name));
        let ordinal = options
            .iter()
            .position(|item| item == value)
            .ok_or_else(|| invalid(path, format!("invalid {name} {value}")))?
            as u32;
        data.word(
            start + offset,
            if name == "DamageFXType" {
                ordinal.wrapping_sub(1)
            } else {
                ordinal
            },
        );
    }
    super::secondary_damage::write_status_flags(
        node.attribute("ObjectStatusForbiddenForDamage")
            .unwrap_or(""),
        start + 0x24,
        data,
    )?;
    for attr in node.attributes() {
        let (kind, offset) = match attr.name() {
            "FXParticleSystemTemplate" => ("FXParticleSystemTemplate", 0x48),
            "NodeFXList" => ("FXList", 0x4c),
            _ => continue,
        };
        let ordinal =
            super::passive_area::reference(attr.value(), kind, path, references, reference_types)?;
        data.word(start + offset, ordinal);
    }
    entries.extend(super::draws::fields(start, &FIELDS));
    let mut children = significant_children(node).peekable();
    let filter = match children.next_if(|n| n.has_tag_name((SAGE_NS, "ObjectFilter"))) {
        Some(child) => Target::Table(super::parking::encode_filter(
            child,
            start + 0x50,
            data,
            reference_types,
        )?),
        None => Target::Null,
    };
    let status = match children.next_if(|n| n.has_tag_name((SAGE_NS, "ObjectStatusValidation"))) {
        Some(child) => Target::Table(super::secondary_damage::encode_status(
            child,
            start + 0x54,
            data,
        )?),
        None => Target::Null,
    };
    if children.next().is_some() {
        return Err(invalid(path, "unsupported LineSegmentDamageUpdate child"));
    }
    entries.extend([
        Entry::named("ObjectFilter", 0xe46e_a3b2, filter),
        Entry::named("ObjectStatusValidation", 0x3e8f_938c, status),
    ]);
    Ok((KIND, Table::record(entries)))
}
