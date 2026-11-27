use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_int, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use super::parking::encode_filter;
use super::passive_area::reference;
use super::secondary_damage::write_status_flags;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x4ca3_7f3e;
    const NUMBERS: [(&str, usize, &str); 3] = [
        ("MaxScanRange", 8, "1"),
        ("MinScanRange", 0xc, "0"),
        ("Radius", 0x10, "0"),
    ];
    const FLAGS: [(&str, usize); 4] = [
        ("StartsActive", 0x40),
        ("UseStartPosition", 0x41),
        ("AdjustAttackPos", 0x42),
        ("AllowSelf", 0x43),
    ];
    let path = data.path;
    let allowed: Vec<_> = ["id", "IdleSeconds", "SpecialAbilityName", "ForbiddenStatus"]
        .into_iter()
        .chain(NUMBERS.map(|(name, ..)| name))
        .chain(FLAGS.map(|(name, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x44)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, offset, default) in NUMBERS {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(
            name,
            0x6458_64f3,
            Target::Data(start + offset),
        ));
    }
    let time = sage_time(
        node.attribute("IdleSeconds").unwrap_or("0s"),
        "IdleSeconds",
        path,
    )?;
    data.word(start + 0x14, time.to_bits());
    let ability = reference(
        node.attribute("SpecialAbilityName").unwrap_or(""),
        "SpecialPowerTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 0x18, ability);
    write_status_flags(
        node.attribute("ForbiddenStatus").unwrap_or(""),
        start + 0x1c,
        data,
    )?;
    entries.extend(fields(
        start,
        &[
            ("IdleSeconds", 0x9986_69e8, 0x14),
            ("SpecialAbilityName", 0xaee8_3888, 0x18),
            ("ForbiddenStatus", 0xa7bd_811e, 0x1c),
        ],
    ));
    for (name, offset) in FLAGS {
        data.payload[start + offset] = boolean(node, name, false, path)?;
        entries.push(Entry::named(
            name,
            0x77e3_f4f7,
            Target::Data(start + offset),
        ));
    }
    let queries: Vec<_> = significant_children(node).collect();
    if queries.len() > 6 {
        return Err(invalid(path, "AutoAbility allows up to six Queries"));
    }
    let count = start + 0x38;
    let first = data.list(count, queries.len(), 12)?;
    let mut items = Vec::with_capacity(queries.len());
    for (index, query) in queries.into_iter().enumerate() {
        if !query.has_tag_name((SAGE_NS, "Queries")) {
            return Err(invalid(path, "expected AutoAbility Queries"));
        }
        validate_attributes(query, &["EMax", "Value"], path)?;
        let start = first + index * 12;
        for (name, default, offset) in [("EMax", "6", 0), ("Value", "-1", 4)] {
            data.word(
                start + offset,
                sage_int(query.attribute(name).unwrap_or(default), name, path)?,
            );
        }
        let mut children = significant_children(query);
        let filter = match children.next() {
            Some(child)
                if child.has_tag_name((SAGE_NS, "ObjectFilter")) && children.next().is_none() =>
            {
                Target::Table(encode_filter(child, start + 8, data, reference_types)?)
            }
            Some(_) => return Err(invalid(path, "unexpected AutoAbility Queries child")),
            None => Target::Null,
        };
        let mut query: Vec<_> = fields(
            start,
            &[("EMax", 0x23e6_50b8, 0), ("Value", 0x23e6_50b8, 4)],
        )
        .collect();
        query.push(Entry::named("ObjectFilter", 0xe46e_a3b2, filter));
        items.push((0, Target::Table(Table::record(query))));
    }
    entries.push(Entry::named(
        "Queries",
        0x1827_c9b4,
        Target::Table(Table::array(count, items)),
    ));
    Ok((KIND, Table::record(entries)))
}
