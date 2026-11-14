use super::binary::BinaryData;
use super::tokenized::{self, Entry, Table, Target};
use super::{Marshaled, finite_number, sage_time, significant_children, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::{Document, Node};
use std::{io, path::Path, sync::OnceLock};

// Read the fixed token table and default fields from the empty EA fixture.
const BASE: &[u8] = include_bytes!(
    "../../../tests/fixtures/ea-batches/weapon-template-root-assets/94d4d96e.0ce6168e.a529f9d4.226acfbb.asset"
);

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ROOT: usize = 0x514;
    const ATTRS: [(&str, usize, &str); 3] = [
        ("AttackRange", 0, "0"),
        ("MinimumAttackRange", 4, "0"),
        ("WeaponSpeed", 0x28, "99999"),
    ];
    validate_attributes(
        node,
        &[
            "id",
            "AttackRange",
            "MinimumAttackRange",
            "WeaponSpeed",
            "ClipSize",
            "CanFireWhileMoving",
            "Flags",
            "RequiredAntiMask",
            "RadiusDamageAffects",
        ],
        path,
    )?;
    let mut data = BinaryData {
        payload: BASE[32 + ROOT..].to_vec(),
        relocations: Vec::new(),
        path,
    };
    let mut table = Table::from_layout(&BASE[32..32 + ROOT], &data.payload, path)?;
    for (name, offset, default) in ATTRS {
        data.word(
            offset,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
    }
    let clip = node
        .attribute("ClipSize")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid WeaponTemplate ClipSize"))?;
    data.word(0x6c, clip as u32);
    data.payload[0x126] = match node
        .attribute("CanFireWhileMoving")
        .unwrap_or("false")
        .trim()
    {
        "true" | "1" => 1,
        "false" | "0" => 0,
        _ => return Err(invalid(path, "invalid CanFireWhileMoving")),
    };
    for (name, kind, offset) in [
        ("Flags", 0, 0xa8),
        ("RadiusDamageAffects", 1, 0xc0),
        ("RequiredAntiMask", 2, 0xcc),
    ] {
        if let Some(value) = node.attribute(name) {
            data.word(offset, weapon_flags(value, kind, path)?);
        }
    }
    let mut children = significant_children(node).peekable();
    for (name, offset) in [
        ("PreAttackDelay", 0xe8),
        ("FiringDuration", 0xec),
        ("CoolDownDelayBetweenShots", 0xf0),
        ("ClipReloadTime", 0xf4),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            validate_attributes(child, &["MinSeconds", "MaxSeconds"], path)?;
            if has_content(child) {
                return Err(invalid(path, format!("unexpected {name} content")));
            }
            let start = data.allocate(offset, 8)?;
            let mut entries = Vec::new();
            for (field, offset) in [("MinSeconds", start), ("MaxSeconds", start + 4)] {
                data.word(
                    offset,
                    sage_time(child.attribute(field).unwrap_or("0s"), field, path)?.to_bits(),
                );
                entries.push(Entry::named(field, 0x9986_69e8, Target::Data(offset)));
            }
            if !table.replace_target(name, Target::Table(Table::record(entries))) {
                return Err(invalid(path, format!("weapon layout missing {name}")));
            }
        }
    }
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!(
                "unsupported WeaponTemplate child {}",
                child.tag_name().name()
            ),
        ));
    }
    tokenized::finish(table, data, Marshaled::default())
}

fn weapon_flags(value: &str, kind: usize, path: &Path) -> io::Result<u32> {
    static FLAGS: OnceLock<[Vec<String>; 3]> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        let schema = Document::parse(include_str!(
            "../../../schemas/ra3/xsd/AssetTypeWeaponTemplate.xsd"
        ))
        .unwrap();
        ["WeaponFlagsType", "WeaponAffectsType", "WpnAntiT"].map(|name| {
            schema
                .descendants()
                .find(|node| node.attribute("name") == Some(name))
                .unwrap()
                .descendants()
                .filter_map(|node| node.attribute("value").map(str::to_owned))
                .collect()
        })
    });
    value.split_ascii_whitespace().try_fold(0, |mask, flag| {
        let bit = flags[kind]
            .iter()
            .position(|name| name == flag)
            .filter(|&bit| bit < 32)
            .ok_or_else(|| invalid(path, format!("invalid weapon flag {flag}")))?;
        Ok(mask | 1 << bit)
    })
}
