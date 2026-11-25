use super::super::binary::BinaryData;
use super::super::schema_enums;
use super::super::tokenized::{Entry, Table, Target};
use super::draws::{boolean, fields};
use super::spawn::{id_list, weak_reference};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, sync::OnceLock};

#[derive(Clone, Copy)]
pub(super) enum TransformUpgrade {
    Armor,
    Geometry,
    RemoveUpgrade,
}

impl TransformUpgrade {
    pub(super) fn lookup(name: &str) -> Option<Self> {
        match name {
            "ArmorUpgrade" => Some(Self::Armor),
            "GeometryUpgrade" => Some(Self::Geometry),
            "RemoveUpgradeUpgrade" => Some(Self::RemoveUpgrade),
            _ => None,
        }
    }

    pub(super) fn kind(self) -> u32 {
        match self {
            Self::Armor => 0xa1fe_37ac,
            Self::Geometry => 0xff84_fbde,
            Self::RemoveUpgrade => 0x75a0_90b5,
        }
    }

    pub(super) fn size(self) -> usize {
        match self {
            Self::Armor => 0x2c,
            Self::Geometry => 0x34,
            Self::RemoveUpgrade => 0x38,
        }
    }

    pub(super) fn attributes(self) -> &'static [&'static str] {
        match self {
            Self::Armor => &["KillArmorUpgrade", "IgnoreArmorUpgrade", "ArmorSetFlag"],
            Self::Geometry => &["ShowGeometry", "HideGeometry"],
            Self::RemoveUpgrade => &[
                "UpradesGroupsToRemove",
                "SuppressEvaEventForRemoval",
                "RemoveFromAllPlayerObjects",
            ],
        }
    }

    pub(super) fn encode<'a, 'input>(
        self,
        node: Node<'a, 'input>,
        mut children: impl Iterator<Item = Node<'a, 'input>>,
        start: usize,
        data: &mut BinaryData<'_>,
        reference_types: &mut Vec<u32>,
    ) -> io::Result<Vec<Entry>> {
        match self {
            Self::Armor => {
                if children.next().is_some() {
                    return Err(invalid(data.path, "unexpected ArmorUpgrade child"));
                }
                static ARMOR_SET_TYPES: OnceLock<[Vec<String>; 1]> = OnceLock::new();
                let types = &ARMOR_SET_TYPES.get_or_init(|| {
                    schema_enums(
                        include_str!("../../../../schemas/ra3/xsd/AssetTypeArmorTemplate.xsd"),
                        ["ArmorSetType"],
                    )
                })[0];
                let flag = node.attribute("ArmorSetFlag").unwrap_or("INVALID");
                let flag = types
                    .iter()
                    .position(|kind| kind == flag)
                    .ok_or_else(|| invalid(data.path, format!("invalid ArmorSetFlag {flag:?}")))?;
                data.word(start + 0x24, flag as u32);

                let flags = [
                    ("KillArmorUpgrade", 0x77e3_f4f7, 0x28),
                    ("IgnoreArmorUpgrade", 0x77e3_f4f7, 0x29),
                ];
                for (name, _, offset) in flags {
                    data.payload[start + offset] = boolean(node, name, false, data.path)?;
                }
                let mut entries: Vec<_> = fields(start, &flags).collect();
                entries.push(Entry::named(
                    "ArmorSetFlag",
                    0x2037_048f,
                    Target::Data(start + 0x24),
                ));
                Ok(entries)
            }
            Self::Geometry => {
                if children.next().is_some() {
                    return Err(invalid(data.path, "unexpected GeometryUpgrade child"));
                }
                ["ShowGeometry", "HideGeometry"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let table = strings(node, name, start + 0x24 + index * 8, data)?;
                        Ok(Entry::named(name, 0x6d66_ae18, Target::Table(table)))
                    })
                    .collect()
            }
            Self::RemoveUpgrade => {
                let mut removals = Vec::new();
                for child in children {
                    if !child.has_tag_name((SAGE_NS, "UpgradeToRemove")) {
                        return Err(invalid(data.path, "unexpected RemoveUpgradeUpgrade child"));
                    }
                    removals.push(weak_reference(
                        child,
                        "UpgradeTemplate",
                        data,
                        reference_types,
                    )?);
                }
                let groups = strings(node, "UpradesGroupsToRemove", start + 0x24, data)?;
                let removals = id_list(data, start + 0x2c, removals)?;
                let mut entries = vec![Entry::named(
                    "UpradesGroupsToRemove",
                    0xe798_0c56,
                    Target::Table(groups),
                )];
                for (index, name) in ["SuppressEvaEventForRemoval", "RemoveFromAllPlayerObjects"]
                    .into_iter()
                    .enumerate()
                {
                    let offset = start + 0x34 + index;
                    data.payload[offset] = boolean(node, name, false, data.path)?;
                    entries.push(Entry::named(name, 0x77e3_f4f7, Target::Data(offset)));
                }
                entries.push(Entry::named(
                    "UpgradeToRemove",
                    0x26fa_cfc7,
                    Target::Table(removals),
                ));
                Ok(entries)
            }
        }
    }
}

fn strings(
    node: Node<'_, '_>,
    name: &str,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let items = node.attribute(name).unwrap_or("").split_ascii_whitespace();
    let start = data.list(count, items.clone().count(), 8)?;
    let mut entries = Vec::new();
    for (index, item) in items.enumerate() {
        let offset = start + index * 8;
        data.string(offset, item)?;
        entries.push((0, Target::Data(offset)));
    }
    Ok(Table::array(count, entries))
}
