use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, fast_hash, finite_number, hash_name, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, inherited};
use super::draws::fields;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x0d96_a2f4;
const REAL: u32 = 0x6458_64f3;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "AssociatedModule",
            "MaxDebrisCloudsPerSecond",
            "FillerMesh",
        ],
        path,
    )?;
    let mut children = significant_children(node);
    let die = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
        .ok_or_else(|| invalid(path, "BuildingDestructionBehavior requires DieMuxData"))?;
    let items: Vec<_> = children.collect();
    if items.is_empty()
        || items
            .iter()
            .any(|child| !child.has_tag_name((SAGE_NS, "Item")))
    {
        return Err(invalid(
            path,
            "BuildingDestructionBehavior requires Item children after DieMuxData",
        ));
    }
    let start = data.allocate(pointer, 0x48)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    if let Some(name) = node
        .attribute("AssociatedModule")
        .filter(|name| !name.is_empty())
    {
        data.word(start + 0x34, hash_name(name));
    }
    let clouds = finite_number(
        node.attribute("MaxDebrisCloudsPerSecond").unwrap_or("2"),
        "MaxDebrisCloudsPerSecond",
        path,
    )?;
    data.word(start + 0x38, clouds.to_bits());
    let mesh = node.attribute("FillerMesh").unwrap_or("NEWSKIN_FILL");
    if !mesh.is_empty() {
        data.word(start + 0x3c, hash_name(mesh));
    }
    let die = encode_die_mux(die, start + 8, data)?;
    let count = start + 0x40;
    let first = data.list(count, items.len(), 0x14)?;
    let mut tokens = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let fields = encode_item(
            item,
            first + index * 0x14,
            data,
            references,
            reference_types,
        )?;
        tokens.push((0, Target::Table(fields)));
    }
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let die = Table::record(vec![
        base(0xa50d_729a, inherited(0xd75d_93a2, module)),
        Entry::named("DieMuxData", 0x0084_001b, Target::Table(die)),
    ]);
    let mut entries = vec![base(0x9e4f_2b72, die)];
    entries.extend(fields(
        start,
        &[
            ("AssociatedModule", 0xbdf0_7965, 0x34),
            ("MaxDebrisCloudsPerSecond", REAL, 0x38),
            ("FillerMesh", 0x03a2_8c40, 0x3c),
        ],
    ));
    entries.push(Entry::named(
        "Item",
        0xffdd_ec52,
        Target::Table(Table::array(count, tokens)),
    ));
    Ok((KIND, Table::record(entries)))
}

fn encode_item(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        node,
        &["BonePrefix", "FX", "CreationObject", "FlingVelocity"],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(
            path,
            "BuildingDestructionBehavior Item cannot contain content",
        ));
    }
    if let Some(prefix) = node
        .attribute("BonePrefix")
        .filter(|prefix| !prefix.is_empty())
    {
        data.string(start, prefix)?;
    }
    for attribute in node.attributes() {
        let value = attribute.value();
        if value.trim().is_empty() {
            continue;
        }
        match attribute.name() {
            "FX" => {
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, "FXList", path)?;
                references.push(id.into());
                reference_types.push(id.type_id);
                data.word(start + 8, references.len() as u32);
            }
            "CreationObject" => {
                let id = asset_reference(value, "GameObject", path)?;
                reference_types.push(fast_hash(b"GameObject"));
                data.word(start + 0xc, id.instance_id);
            }
            _ => {}
        }
    }
    let velocity = finite_number(
        node.attribute("FlingVelocity").unwrap_or("1"),
        "FlingVelocity",
        path,
    )?;
    data.word(start + 0x10, velocity.to_bits());
    Ok(Table::record(
        fields(
            start,
            &[
                ("BonePrefix", 0x1a29_33aa, 0),
                ("FX", 0x0d1c_8e83, 8),
                ("CreationObject", 0x148e_d75d, 0xc),
                ("FlingVelocity", REAL, 0x10),
            ],
        )
        .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn building_destruction_matches_ea() {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-building-destruction.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            document.root_element().first_element_child().unwrap(),
            Path::new("game-object-building-destruction.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-building-destruction.bin");
        let relo =
            include_bytes!("../../../../tests/fixtures/game-object-building-destruction.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-building-destruction.manifest"
        ))
        .unwrap();
        assert_eq!(actual.payload.len(), bin.len() - 4);
        assert_eq!(
            actual
                .payload
                .iter()
                .zip(&bin[4..])
                .position(|(a, b)| a != b),
            None
        );
        assert_eq!(actual.relocations, relo[4..]);
        assert!(actual.imports.is_empty());
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.assets[0].references.iter())
        );
    }
}
