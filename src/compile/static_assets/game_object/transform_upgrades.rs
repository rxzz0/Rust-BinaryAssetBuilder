use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::draws::boolean;
use super::spawn::{id_list, weak_reference};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

#[derive(Clone, Copy)]
pub(super) enum TransformUpgrade {
    Geometry,
    RemoveUpgrade,
}

impl TransformUpgrade {
    pub(super) fn lookup(name: &str) -> Option<Self> {
        match name {
            "GeometryUpgrade" => Some(Self::Geometry),
            "RemoveUpgradeUpgrade" => Some(Self::RemoveUpgrade),
            _ => None,
        }
    }

    pub(super) fn kind(self) -> u32 {
        match self {
            Self::Geometry => 0xff84_fbde,
            Self::RemoveUpgrade => 0x75a0_90b5,
        }
    }

    pub(super) fn size(self) -> usize {
        match self {
            Self::Geometry => 0x34,
            Self::RemoveUpgrade => 0x38,
        }
    }

    pub(super) fn attributes(self) -> &'static [&'static str] {
        match self {
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
