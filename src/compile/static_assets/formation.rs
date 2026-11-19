use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::game_object::encode_inline_object_filter;
use super::{
    Marshaled, asset_reference, finite_number, sage_uint, significant_children, text_value,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use crate::hash::fast_hash;
use roxmltree::Node;
use std::{io, path::Path};

const FILTER_SIZE: usize = 112;

pub(super) fn assistant(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const NAMES: &[&str] = &[
        "id",
        "DefaultPreviewObject",
        "FacingArrowTemplateHead",
        "FacingArrowTemplateBody",
        "FacingArrowTemplateBase",
        "RowPadding",
        "ColumnPadding",
        "ActivateDragDistance",
        "ActivateTime",
    ];
    validate_attributes(node, NAMES, path)?;
    let (mut data, mut result) = marshaler(40, path);
    for attribute in node.attributes() {
        if let Some(index) = NAMES[1..5]
            .iter()
            .position(|&name| name == attribute.name())
        {
            let id = asset_reference(attribute.value(), "GameObject", path)?;
            data.word(4 + index * 4, id.instance_id);
            result.reference_types.push(fast_hash(b"GameObject"));
        }
    }
    for (index, &name) in NAMES[5..].iter().enumerate() {
        data.word(
            20 + index * 4,
            finite_number(required(node, name, path)?, name, path)?.to_bits(),
        );
    }
    let mut children = significant_children(node);
    if let Some(filter) = children.next() {
        if !filter.has_tag_name((SAGE_NS, "ObjectFilter")) || children.next().is_some() {
            return Err(invalid(
                path,
                "expected one FormationAssistant ObjectFilter",
            ));
        }
        let start = data.allocate(36, FILTER_SIZE)?;
        encode_inline_object_filter(filter, start, &mut data, &mut result.reference_types)?;
    }
    Ok(data.finish(result))
}

pub(super) fn unit(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let (mut data, mut result) = marshaler(24, path);
    let mut children = significant_children(node).peekable();
    for (name, offset) in [("IncludeObjectFilter", 4), ("ExcludeObjectFilter", 12)] {
        let mut filters = Vec::new();
        while children
            .peek()
            .is_some_and(|child| child.has_tag_name((SAGE_NS, name)))
        {
            filters.push(children.next().expect("peeked filter"));
        }
        if offset == 4 && filters.is_empty() {
            return Err(invalid(path, "expected at least one IncludeObjectFilter"));
        }
        let start = data.list(offset, filters.len(), FILTER_SIZE)?;
        for (index, filter) in filters.into_iter().enumerate() {
            encode_inline_object_filter(
                filter,
                start + index * FILTER_SIZE,
                &mut data,
                &mut result.reference_types,
            )?;
        }
    }
    if let Some(preview) = children.next() {
        if !preview.has_tag_name((SAGE_NS, "PreviewObject")) || children.next().is_some() {
            return Err(invalid(
                path,
                "unexpected FormationAssistantUnitDefinition child",
            ));
        }
        let id = asset_reference(text_value(preview, path)?, "GameObject", path)?;
        data.optional_word(20, id.instance_id)?;
        result.reference_types.push(fast_hash(b"GameObject"));
    }
    data.relocations.sort_unstable();
    Ok(data.finish(result))
}

pub(super) fn template(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let (mut data, mut result) = marshaler(12, path);
    let rows = required_list(node, "Row", path)?;
    let row_start = data.list(4, rows.len(), 8)?;
    for (i, row) in rows.into_iter().enumerate() {
        let cells = required_list(row, "Cell", path)?;
        let cell_start = data.list(row_start + i * 8, cells.len(), 8)?;
        for (j, cell) in cells.into_iter().enumerate() {
            let definitions = required_list(cell, "Definition", path)?;
            let definition_start = data.list(cell_start + j * 8, definitions.len(), 4)?;
            for (k, definition) in definitions.into_iter().enumerate() {
                write_import(
                    text_value(definition, path)?,
                    "FormationAssistantUnitDefinition",
                    definition_start + k * 4,
                    &mut data,
                    &mut result,
                )?;
            }
        }
    }
    data.relocations.sort_unstable();
    Ok(data.finish(result))
}

pub(super) fn selection(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const TARGET: &str = "FormationAssistantTemplate";
    const LENGTH: &str = "MaxDragLength";
    const UNITS: &str = "MaxUnitsSelected";
    validate_leaf(node, &["id", TARGET, LENGTH, UNITS], path)?;
    let (mut data, mut result) = marshaler(16, path);
    write_import(
        required(node, TARGET, path)?,
        TARGET,
        4,
        &mut data,
        &mut result,
    )?;
    data.word(
        8,
        finite_number(required(node, LENGTH, path)?, LENGTH, path)?.to_bits(),
    );
    data.word(12, sage_uint(required(node, UNITS, path)?, UNITS, path)?);
    Ok(data.finish(result))
}

fn marshaler(size: usize, path: &Path) -> (BinaryData<'_>, Marshaled) {
    (
        BinaryData {
            payload: vec![0; size],
            relocations: Vec::new(),
            path,
        },
        Marshaled::default(),
    )
}

fn required_list<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    path: &Path,
) -> io::Result<Vec<Node<'a, 'input>>> {
    let attributes: &[&str] = if name == "Row" { &["id"] } else { &[] };
    validate_attributes(node, attributes, path)?;
    let children: Vec<_> = significant_children(node).collect();
    if children.is_empty()
        || children
            .iter()
            .any(|child| !child.has_tag_name((SAGE_NS, name)))
    {
        return Err(invalid(
            path,
            format!("expected one or more {name} elements"),
        ));
    }
    Ok(children)
}
