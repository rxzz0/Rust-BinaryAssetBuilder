use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, hash_name, required_element, significant_children, text_value,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::boolean;
use super::passive_area::reference;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) const KIND: u32 = 0x6002_0441;
const BOOL: u32 = 0x77e3_f4f7;
const REAL: u32 = 0x6458_64f3;
const INT: u32 = 0x23e6_50b8;
const HIT_FX: u32 = 0x0d1c_8e83;
const FIELDS: [(&str, u32, usize, &str); 9] = [
    ("UseDistortionShader", BOOL, 0x48, "false"),
    ("Velocity", REAL, 8, "200"),
    ("ArcHeightFactor", REAL, 0xc, "0"),
    ("UVWorldSize", REAL, 0x10, "10"),
    ("NumTubeSides", INT, 0x14, "8"),
    ("TubeRadius", REAL, 0x18, "1"),
    ("HitFx", HIT_FX, 0x1c, ""),
    ("WeaponSlotID", INT, 0x20, "-1"),
    ("StreamStateID", INT, 0x24, "0"),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, _, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let shader = required_element(&mut children, "FXShader", path)?;
    let particles = children.next();
    if particles.is_some_and(|child| !child.has_tag_name((SAGE_NS, "ParticleSystems")))
        || children.next().is_some()
    {
        return Err(invalid(path, "unexpected StreamDraw child"));
    }
    let start = data.allocate(pointer, 0x4c)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, INT, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut entries = vec![base(0x17f6_1728, inherited(0xd75d_93a2, module))];
    for (name, kind, offset, default) in FIELDS {
        let field = start + offset;
        let value = node.attribute(name).unwrap_or(default);
        match kind {
            BOOL => data.payload[field] = boolean(node, name, false, path)?,
            REAL => data.word(field, finite_number(value, name, path)?.to_bits()),
            INT => {
                let number = value
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?;
                data.word(field, number as u32);
            }
            HIT_FX => data.word(
                field,
                reference(value, "FXList", path, references, reference_types)?,
            ),
            _ => unreachable!("StreamDraw field type"),
        }
        entries.push(Entry::named(name, kind, Target::Data(field)));
    }
    let shader = super::material::encode_inline(
        shader,
        start + 0x28,
        data,
        references,
        reference_types,
        float_names,
    )?;
    let particles = match particles {
        Some(node) => Target::Table(encode_particles(
            node,
            start + 0x44,
            data,
            references,
            reference_types,
        )?),
        None => Target::Null,
    };
    entries.extend([
        Entry::named("FXShader", 0xc7ea_3cf4, Target::Table(shader)),
        Entry::named("ParticleSystems", 0xb26f_4cd7, particles),
    ]);
    Ok(Table::record(entries))
}

fn encode_particles(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(node, &[], path)?;
    let children: Vec<_> = significant_children(node).collect();
    let start = data.allocate(pointer, 8)?;
    let first = data.list(start, children.len(), 4)?;
    let mut items = Vec::with_capacity(children.len());
    for (index, child) in children.into_iter().enumerate() {
        if !child.has_tag_name((SAGE_NS, "ParticleSys")) {
            return Err(invalid(path, "unexpected StreamDraw ParticleSystems child"));
        }
        let value = text_value(child, path)?;
        let ordinal = reference(
            value,
            "FXParticleSystemTemplate",
            path,
            references,
            reference_types,
        )?;
        let field = first + 4 * index;
        data.word(field, ordinal);
        items.push((0, Target::Data(field)));
    }
    Ok(Table::record(vec![Entry::named(
        "ParticleSys",
        0x43f1_ce36,
        Target::Table(Table::array(start, items)),
    )]))
}
