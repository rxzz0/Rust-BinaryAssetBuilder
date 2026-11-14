use super::binary::BinaryData;
use super::{
    Marshaled, asset_reference, finite_number, sage_time, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::{Document, Node};
use std::{io, path::Path, sync::OnceLock};

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const FX: [&str; 10] = [
        "MajorFX",
        "MinorFX",
        "RearFX",
        "SideFX",
        "TopFX",
        "VeteranMajorFX",
        "VeteranMinorFX",
        "VeteranRearFX",
        "VeteranSideFX",
        "VeteranTopFX",
    ];
    static TYPES: OnceLock<Vec<String>> = OnceLock::new();
    let kinds = TYPES.get_or_init(|| {
        Document::parse(include_str!(
            "../../../schemas/ra3/xsd/Includes/DamageFX.xsd"
        ))
        .unwrap()
        .descendants()
        .find(|node| node.attribute("name") == Some("DamageFXType"))
        .unwrap()
        .descendants()
        .filter_map(|node| node.attribute("value").map(str::to_owned))
        .collect()
    });
    validate_attributes(node, &["id"], path)?;
    let allowed: Vec<_> = ["Type", "Amount", "ThrottleTime"]
        .into_iter()
        .chain(FX)
        .collect();
    let groups: Vec<_> = significant_children(node).collect();
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let first = data.list(4, groups.len(), 52)?;
    let mut result = Marshaled::default();
    for (index, group) in groups.into_iter().enumerate() {
        if !group.has_tag_name((SAGE_NS, "FXGroup")) || has_content(group) {
            return Err(invalid(path, "expected an empty DamageFX FXGroup"));
        }
        validate_attributes(group, &allowed, path)?;
        let start = first + index * 52;
        let kind = group.attribute("Type").unwrap_or("DEFAULT");
        let value = kinds
            .iter()
            .position(|item| item == kind)
            .ok_or_else(|| invalid(path, format!("invalid DamageFX Type {kind}")))?;
        data.word(start, (value as u32).wrapping_sub(1));
        data.word(
            start + 4,
            finite_number(group.attribute("Amount").unwrap_or("0"), "Amount", path)?.to_bits(),
        );
        data.word(
            start + 8,
            sage_time(
                group.attribute("ThrottleTime").unwrap_or("0s"),
                "ThrottleTime",
                path,
            )?
            .to_bits(),
        );
        for attr in group.attributes() {
            if let Some(index) = FX
                .iter()
                .position(|&field| field == attr.name())
                .filter(|_| !attr.value().trim().is_empty())
            {
                let offset = start + 12 + index * 4;
                let value = attr
                    .value()
                    .split_once('\\')
                    .map_or(attr.value(), |(name, _)| name);
                let id = asset_reference(value, "FXList", path)?;
                result.references.push(id.into());
                result.reference_types.push(id.type_id);
                result.imports.push(offset as u32);
                data.word(offset, result.references.len() as u32);
            }
        }
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}
