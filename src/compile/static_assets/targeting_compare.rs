use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::{
    Marshaled, encode_words, finite_number, significant_children, text_value, validate_attributes,
    write_import,
};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, path::Path};

pub(in crate::compile) const BASE: &str = "TargetingBaseCompare";
pub(in crate::compile) const TYPES: &[&str] = &[
    "TargetingDistanceCompare",
    "TargetingCombatChainCompare",
    "TargetingInTurretArcCompare",
];

pub(super) fn distance(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", "Tolerance"], path)?;
    let tolerance = finite_number(
        node.attribute("Tolerance").unwrap_or("30.0"),
        "Tolerance",
        path,
    )?;
    Ok(Marshaled {
        payload: encode_words([0, tolerance.to_bits()]),
        ..Marshaled::default()
    })
}

pub(super) fn marker(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id"], path)?;
    Ok(Marshaled {
        payload: vec![0; 4],
        ..Marshaled::default()
    })
}

pub(super) fn list(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let comparisons: Vec<_> = significant_children(node).collect();
    if comparisons.is_empty() {
        return Err(invalid(path, "expected at least one targeting Compare"));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let start = data.list(4, comparisons.len(), 4)?;
    for (index, comparison) in comparisons.into_iter().enumerate() {
        if !comparison.has_tag_name((SAGE_NS, "Compare")) {
            return Err(invalid(path, "expected a targeting Compare"));
        }
        write_import(
            text_value(comparison, path)?,
            BASE,
            start + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    Ok(data.finish(result))
}
