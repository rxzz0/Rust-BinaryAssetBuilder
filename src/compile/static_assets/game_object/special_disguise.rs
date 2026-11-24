use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{asset_reference, schema_enums, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::{io, sync::OnceLock};

pub(super) const KIND: u32 = 0xa5fa_c7df;
pub(super) const SIZE: usize = 0x108;
pub(super) const FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    (
        "TriggerInstantlyOnCreate",
        0x77e3_f4f7,
        0x104,
        "bool",
        "false",
    ),
    ("OpacityTarget", 0x6458_64f3, 0xf0, "real", "0"),
    ("DisguiseFX", 0x0d1c_8e83, 0xf4, "ref", "FXList"),
    ("ScanRadiusForPropDisguise", 0x6458_64f3, 0xf8, "real", "0"),
];

pub(super) fn terrain<'a, 'input: 'a>(
    children: impl Iterator<Item = Node<'a, 'input>>,
    start: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Entry> {
    static CLASSES: OnceLock<[Vec<String>; 1]> = OnceLock::new();
    let classes = &CLASSES.get_or_init(|| {
        schema_enums(
            include_str!("../../../../schemas/ra3/xsd/AssetTypeTerrainAsset.xsd"),
            ["TerrainClassType"],
        )
    })[0];

    const ITEM_SIZE: usize = 8;
    let children: Vec<_> = children.collect();
    let count = start + 0xfc;
    let first = data.list(count, children.len(), ITEM_SIZE)?;
    let mut items = Vec::with_capacity(children.len());
    let path = data.path;
    for (index, child) in children.into_iter().enumerate() {
        if !child.has_tag_name((SAGE_NS, "TerrainData")) || has_content(child) {
            return Err(invalid(path, "expected SpecialDisguiseUpdate TerrainData"));
        }
        validate_attributes(child, &["TerrainClass", "ObjectTemplate"], path)?;
        let terrain = child.attribute("TerrainClass").unwrap_or("UNSPECIFIED");
        let terrain = classes
            .iter()
            .position(|class| class == terrain)
            .ok_or_else(|| invalid(path, format!("invalid TerrainClass {terrain:?}")))?;
        let start = first + index * ITEM_SIZE;
        data.word(start, terrain as u32);
        if let Some(template) = child
            .attribute("ObjectTemplate")
            .filter(|value| !value.is_empty())
        {
            let template = asset_reference(template, "GameObject", path)?;
            data.word(start + 4, template.instance_id);
            reference_types.push(template.type_id);
        }
        let fields = Table::record(vec![
            Entry::named("TerrainClass", 0xcde3_2fd2, Target::Data(start)),
            Entry::named("ObjectTemplate", 0x148e_d75d, Target::Data(start + 4)),
        ]);
        items.push((0, Target::Table(fields)));
    }
    Ok(Entry::named(
        "TerrainData",
        0x3959_2ed9,
        Target::Table(Table::array(count, items)),
    ))
}
