use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x6e77_aa59;
    const TIME: u32 = 0x9986_69e8;
    const REAL: u32 = 0x6458_64f3;
    let path = data.path;
    let fields = [
        ("ScanTime", TIME, 8, "0.2s"),
        ("DelayBetweenVictimsTime", TIME, 0xc, "0.3s"),
        ("EffectDistance", REAL, 0x10, "125"),
        ("GatherAcceleration", REAL, 0x14, "0.01"),
        ("ModelLength", REAL, 0x18, "100"),
        ("HeightOffGround", REAL, 0x24, "50"),
    ];
    let refs = [
        ("ArmTemplate", 0xbfab_9075, 0x1c, "GameObject"),
        ("VictimShader", 0x8a16_c611, 0x20, "ShaderOverride"),
    ];
    let allowed: Vec<_> = ["id"]
        .into_iter()
        .chain(fields.map(|(name, ..)| name))
        .chain(refs.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let filter = children
        .next_if(|child| child.has_tag_name((SAGE_NS, "Filter")))
        .ok_or_else(|| invalid(path, "SqueezeFlingUpdate requires Filter"))?;
    let kill = children.next_if(|child| child.has_tag_name((SAGE_NS, "KillFilter")));
    let start = data.allocate(pointer, 0xa4)?;
    let mut entries = vec![super::behaviors_basic::module_base(
        node, KIND, start, data, true,
    )?];
    let mut ordinals = [0; 2];
    for attr in node.attributes() {
        if let Some(i) = refs.iter().position(|&(name, ..)| name == attr.name()) {
            ordinals[i] = super::passive_area::reference(
                attr.value(),
                refs[i].3,
                path,
                references,
                reference_types,
            )?;
        }
    }
    for (name, ty, offset, default) in fields[..5].iter().copied() {
        let value = node.attribute(name).unwrap_or(default);
        let value = if ty == TIME {
            sage_time(value, name, path)?
        } else {
            finite_number(value, name, path)?
        };
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(name, ty, Target::Data(start + offset)));
    }
    for ((name, ty, offset, _), ordinal) in refs.into_iter().zip(ordinals) {
        let target = if node.attribute(name).is_some() {
            let offset = data.allocate(start + offset, 4)?;
            data.word(offset, ordinal);
            Target::Data(offset)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, ty, target));
    }
    let (name, ty, offset, default) = fields[5];
    data.word(
        start + offset,
        finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
    );
    entries.push(Entry::named(name, ty, Target::Data(start + offset)));
    let filter = super::parking::encode_inline_filter(filter, start + 0x28, data, reference_types)?;
    entries.push(Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)));
    let kill = match kill {
        Some(child) => Target::Table(super::parking::encode_filter(
            child,
            start + 0x98,
            data,
            reference_types,
        )?),
        None => Target::Null,
    };
    entries.push(Entry::named("KillFilter", 0xe46e_a3b2, kill));
    let particles: Vec<_> = children.collect();
    let count = start + 0x9c;
    let first = data.list(count, particles.len(), 20)?;
    let mut items = Vec::with_capacity(particles.len());
    for (i, child) in particles.into_iter().enumerate() {
        if !child.has_tag_name((SAGE_NS, "ParticleData")) || has_content(child) {
            return Err(invalid(path, "expected SqueezeFlingUpdate ParticleData"));
        }
        validate_attributes(
            child,
            &["SourceBone", "DestinationBone", "ParticleSystem"],
            path,
        )?;
        let offset = first + i * 20;
        for (name, field) in [("SourceBone", offset), ("DestinationBone", offset + 8)] {
            if let Some(value) = child.attribute(name) {
                data.string(field, value)?;
            }
        }
        let ordinal = super::passive_area::reference(
            child.attribute("ParticleSystem").unwrap_or(""),
            "FXParticleSystemTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(offset + 16, ordinal);
        items.push((
            0,
            Target::Table(Table::record(
                super::draws::fields(
                    offset,
                    &[
                        ("SourceBone", 0x1a29_33aa, 0),
                        ("DestinationBone", 0x1a29_33aa, 8),
                        ("ParticleSystem", 0x43f1_ce36, 16),
                    ],
                )
                .collect(),
            )),
        ));
    }
    entries.push(Entry::named(
        "ParticleData",
        0x5097_218e,
        Target::Table(Table::array(count, items)),
    ));
    Ok((KIND, Table::record(entries)))
}
