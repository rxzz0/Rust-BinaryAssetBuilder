use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, percentage, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x196f_c7e8;
    const REAL: u32 = 0x6458_64f3;
    const TIME: u32 = 0x9986_69e8;
    const WEAPON: u32 = 0xd046_dd22;
    const FIELDS: [(&str, u32, usize, &str); 9] = [
        ("PullRate", REAL, 8, ""),
        ("MinBounceDelay", TIME, 0xc, "0.75s"),
        ("MaxBounceDelay", TIME, 0x10, "1.5s"),
        ("BouncePercentage", REAL, 0x14, "20%"),
        ("CollidingPullRateFactor", REAL, 0x18, "0%"),
        ("SingleFireWeapon", WEAPON, 0x1c, ""),
        ("ContinuousFireWeapon", WEAPON, 0x20, ""),
        ("ContinuousFireWeaponBlocked", WEAPON, 0x24, ""),
        ("ContinuousFireWeaponDelay", TIME, 0x28, "0.5s"),
    ];
    const STATUS: &str = "ObjectStatusValidation";
    const FILTER: &str = "KeepAliveObjectFilter";

    let path = data.path;
    let allowed = ["id"]
        .into_iter()
        .chain(FIELDS.map(|(name, _, _, _)| name))
        .collect::<Vec<_>>();
    validate_attributes(node, &allowed, path)?;
    let pull_rate = finite_number(required(node, "PullRate", path)?, "PullRate", path)?;
    let mut children = significant_children(node).peekable();
    let status = children.next_if(|child| child.has_tag_name((SAGE_NS, STATUS)));
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, FILTER)));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected GrapplingHookUpdate child"));
    }

    let start = data.allocate(pointer, 0x34)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.word(start + 8, pull_rate.to_bits());
    for &(name, kind, offset, default) in &FIELDS[1..] {
        let value = node.attribute(name).unwrap_or(default);
        let word = match kind {
            REAL => percentage(value, name, path)?.to_bits(),
            TIME => sage_time(value, name, path)?.to_bits(),
            WEAPON => super::passive_area::reference(
                value,
                "WeaponTemplate",
                path,
                references,
                reference_types,
            )?,
            _ => unreachable!("GrapplingHookUpdate field type"),
        };
        data.word(start + offset, word);
    }
    entries.extend(fields(
        start,
        &FIELDS.map(|(name, kind, offset, _)| (name, kind, offset)),
    ));
    let status = match status {
        Some(child) => Target::Table(super::secondary_damage::encode_status(
            child,
            start + 0x2c,
            data,
        )?),
        None => Target::Null,
    };
    entries.push(Entry::named(STATUS, 0x3e8f_938c, status));
    let filter = match filter {
        Some(child) => Target::Table(super::parking::encode_filter(
            child,
            start + 0x30,
            data,
            reference_types,
        )?),
        None => Target::Null,
    };
    entries.push(Entry::named(FILTER, 0xe46e_a3b2, filter));
    Ok((KIND, Table::record(entries)))
}
