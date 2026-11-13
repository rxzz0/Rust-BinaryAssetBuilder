use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, significant_children, validate_attributes,
};
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
    float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    let start = data.allocate(pointer, 0x1c)?;
    encode_inline(node, start, data, references, reference_types, float_names)
}

pub(super) fn encode_inline(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        node,
        &["ShaderName", "TechniqueName", "TechniqueIndex"],
        path,
    )?;
    data.string(start, required(node, "ShaderName", path)?)?;
    data.string(start + 8, node.attribute("TechniqueName").unwrap_or(""))?;
    data.payload[start + 0x18] = node
        .attribute("TechniqueIndex")
        .unwrap_or("0")
        .trim()
        .parse()
        .map_err(|_| invalid(path, "invalid Material TechniqueIndex"))?;

    let mut children = significant_children(node);
    let mut items = Vec::new();
    let count = start + 0x10;
    if let Some(constants) = children.next() {
        if !constants.has_tag_name((SAGE_NS, "Constants")) || children.next().is_some() {
            return Err(invalid(path, "Material accepts one Constants child"));
        }
        validate_attributes(constants, &[], path)?;
        let constants: Vec<_> = significant_children(constants).collect();
        let pointers = data.list(count, constants.len(), 4)?;
        items.reserve(constants.len());
        for (index, node) in constants.into_iter().enumerate() {
            let (kind, table) = constant(
                node,
                pointers + index * 4,
                data,
                references,
                reference_types,
                float_names,
            )?;
            items.push((
                0,
                Target::Table(Table::sequence(vec![Entry::new(
                    2,
                    kind,
                    Target::Table(table),
                )])),
            ));
        }
    }
    let mut entries: Vec<_> = fields(
        start,
        &[
            ("ShaderName", 0x1a29_33aa, 0),
            ("TechniqueName", 0x1a29_33aa, 8),
            ("TechniqueIndex", 0x3091_6b96, 0x18),
        ],
    )
    .collect();
    entries.push(Entry::named(
        "Constants",
        0x1984_2b2f,
        Target::Table(Table::array(count, items)),
    ));
    Ok(Table::record(entries))
}

fn constant(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    float_names: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let name = node.tag_name().name();
    let (kind, value_type, limit, size) = match (node.tag_name().namespace(), name) {
        (Some(SAGE_NS), "Texture") => (0xa590_96a6, 0xbdb6_3f16, 1, 16),
        (Some(SAGE_NS), "Float") => (0xdf08_df25, 0x6458_64f3, 4, 20),
        (Some(SAGE_NS), "Int") => (0x8918_1982, 0x23e6_50b8, 1, 16),
        (Some(SAGE_NS), "Bool") => (0xa3f8_4c3d, 0x77e3_f4f7, 1, 16),
        _ => return Err(invalid(path, format!("unsupported shader constant {name}"))),
    };
    validate_attributes(node, &["Name"], path)?;
    let values = significant_children(node)
        .map(|child| {
            if !child.has_tag_name((SAGE_NS, "Value"))
                || child.children().any(|child| child.is_element())
            {
                return Err(invalid(path, "shader constant requires Value text"));
            }
            validate_attributes(child, &[], path)?;
            Ok(child
                .children()
                .filter_map(|child| child.is_text().then(|| child.text()).flatten())
                .collect::<String>())
        })
        .collect::<io::Result<Vec<_>>>()?;
    if values.is_empty() || values.len() > limit {
        return Err(invalid(
            path,
            format!("shader {name} requires 1 to {limit} values"),
        ));
    }

    let start = data.allocate(pointer, size)?;
    data.word(start, kind);
    let value = if name == "Float" {
        let count = start + 12;
        let first = data.list(count, values.len(), 4)?;
        let mut items = Vec::with_capacity(values.len());
        for (index, value) in values.iter().enumerate() {
            let offset = first + index * 4;
            data.word(
                offset,
                finite_number(value, "shader Float", path)?.to_bits(),
            );
            items.push((0, Target::Data(offset)));
        }
        float_names.push((start + 8) as u32);
        Target::Table(Table::array(count, items))
    } else {
        let value = values[0].trim();
        let word = match name {
            "Texture" if value.is_empty() => 0,
            "Texture" => {
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, "Texture", path)?;
                references.push(id.into());
                reference_types.push(id.type_id);
                references.len() as u32
            }
            "Int" => value
                .parse::<i32>()
                .map_err(|_| invalid(path, "invalid shader Int"))? as u32,
            "Bool" => match value {
                "true" | "1" => 1,
                "false" | "0" => 0,
                _ => return Err(invalid(path, "invalid shader Bool")),
            },
            _ => unreachable!(),
        };
        data.word(start + 12, word);
        Target::Data(start + 12)
    };
    data.string(start + 4, required(node, "Name", path)?)?;
    let base = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::named("Name", 0x1a29_33aa, Target::Data(start + 4)),
    ]);
    Ok((
        kind,
        Table::record(vec![
            Entry::new(1, 0x1724_bb05, Target::Table(base)),
            Entry::named("Value", value_type, value),
        ]),
    ))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn draw_material_matches_ea() {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-material.xml"
        ))
        .unwrap();
        let node = document.root_element().first_element_child().unwrap();
        let actual = compile_asset(node, Path::new("game-object-material.xml"), "").unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-material.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-material.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-material.manifest"
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
