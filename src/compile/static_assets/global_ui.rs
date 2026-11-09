use super::{
    Marshaled, asset_reference, encode_words, fast_hash, finite_number, required_element,
    significant_children, text_value, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;

const HEALTH_BAR: &str = "HealthBarTemplate";
const MAPPING_WORDS: usize = 29;

pub(super) fn camera(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const FIELDS: [&str; 20] = [
        "DefaultMinHeight",
        "DefaultMaxHeight",
        "DefaultPitchAngle",
        "DefaultYawAngle",
        "DefaultScrollSpeedScalar",
        "DefaultHeightMapSmoothnessScalar",
        "ScrollAmountCutoff",
        "ScrollPitchMultiplier",
        "CameraAdjustSpeed",
        "ShakeSubtleIntensity",
        "ShakeNormalIntensity",
        "ShakeStrongIntensity",
        "ShakeSevereIntensity",
        "ShakeCineExtremeIntensity",
        "ShakeCineInsaneIntensity",
        "MaxShakeIntensity",
        "MaxShakeRange",
        "CameraLockHeightDelta",
        "CameraTerrainSampleRadiusForHeight",
        "CameraEaseFactor",
    ];
    let attributes: Vec<_> = std::iter::once("id").chain(FIELDS).collect();
    validate_attributes(node, &attributes, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in CameraSettings"));
    }
    let mut words = [0; 21];
    for (word, field) in words[1..].iter_mut().zip(FIELDS) {
        *word = finite_number(node.attribute(field).unwrap_or("0"), field, path)?.to_bits();
    }
    Ok(Marshaled {
        payload: encode_words(words),
        ..Marshaled::default()
    })
}

pub(super) fn health_bar_template(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Length"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in HealthBarTemplate"));
    }
    let length = required(node, "Length", path)?
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid HealthBarTemplate Length"))?;
    Ok(Marshaled {
        payload: encode_words([0, length]),
        ..Marshaled::default()
    })
}

pub(super) fn health_bar_settings(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "DefaultHealthBar"], path)?;
    let mappings: Vec<_> = significant_children(node).collect();
    let mut data = Marshaled::default();
    let mut words = vec![0; 4 + MAPPING_WORDS * mappings.len()];
    if let Some(default) = node.attribute("DefaultHealthBar") {
        words[1] = import_health_bar(&mut data, default, 1, path)?;
    }
    if !mappings.is_empty() {
        words[2] = mappings.len() as u32;
        words[3] = 16;
        data.relocations.push(12);
    }

    let schema = Document::parse(include_str!("../../../schemas/ra3/xsd/Includes/KindOf.xsd"))
        .expect("bundled KindOf schema must be valid");
    let kind_of: Vec<_> = schema
        .root_element()
        .descendants()
        .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "enumeration")))
        .filter_map(|node| node.attribute("value"))
        .collect();
    for (index, mapping) in mappings.into_iter().enumerate() {
        if !mapping.has_tag_name((SAGE_NS, "ObjectMapping")) {
            return Err(invalid(path, "expected a HealthBarSettings ObjectMapping"));
        }
        validate_attributes(mapping, &["HealthBar"], path)?;
        let start = 4 + index * MAPPING_WORDS;
        words[start] = import_health_bar(
            &mut data,
            required(mapping, "HealthBar", path)?,
            start,
            path,
        )?;
        let mut children = significant_children(mapping);
        let filter = required_element(&mut children, "ObjectFilter", path)?;
        if children.next().is_some() {
            return Err(invalid(path, "unexpected content in ObjectMapping"));
        }
        object_filter(filter, start + 1, &kind_of, &mut words, &mut data, path)?;
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    if !data.imports.is_empty() {
        data.imports.push(u32::MAX);
    }
    data.payload = encode_words(words);
    Ok(data)
}

fn import_health_bar(
    data: &mut Marshaled,
    value: &str,
    word: usize,
    path: &Path,
) -> io::Result<u32> {
    let id = asset_reference(value, HEALTH_BAR, path)?;
    data.reference_types.push(id.type_id);
    data.references.push(id.into());
    data.imports.push((4 * word) as u32);
    Ok(data.references.len() as u32)
}

fn object_filter(
    node: Node<'_, '_>,
    start: usize,
    kind_of: &[&str],
    words: &mut Vec<u32>,
    data: &mut Marshaled,
    path: &Path,
) -> io::Result<()> {
    validate_attributes(
        node,
        &["Rule", "Relationship", "Alignment", "Include", "Exclude"],
        path,
    )?;
    words[start + 1] = enum_value(
        node.attribute("Rule").unwrap_or("NONE"),
        &["UNPARSED", "ALL", "ANY", "NONE"],
        path,
    )?;
    words[start + 3] = enum_value(
        node.attribute("Alignment").unwrap_or("NONE"),
        &["NONE", "EVIL", "GOOD"],
        path,
    )?;
    for value in node
        .attribute("Relationship")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        words[start + 2] |= 1
            << enum_value(
                value,
                &["ALLIES", "ENEMIES", "NEUTRAL", "SAME_PLAYER"],
                path,
            )?;
    }
    for (field, base) in [("Include", 4), ("Exclude", 13)] {
        for value in node.attribute(field).unwrap_or("").split_ascii_whitespace() {
            let bit = enum_value(value, kind_of, path)? as usize;
            words[start + base + bit / 32] |= 1 << (bit % 32);
        }
    }
    let mut children = significant_children(node).peekable();
    for (tag, offset) in [("IncludeThing", 24), ("ExcludeThing", 26)] {
        let first = words.len();
        while let Some(item) = children.next_if(|item| item.has_tag_name((SAGE_NS, tag))) {
            let asset = asset_reference(text_value(item, path)?, "GameObject", path)?;
            words.push(asset.instance_id);
            data.reference_types.push(fast_hash(b"GameObject"));
        }
        if words.len() != first {
            words[start + offset] = (words.len() - first) as u32;
            words[start + offset + 1] = (4 * first) as u32;
            data.relocations.push((4 * (start + offset + 1)) as u32);
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in ObjectFilter"));
    }
    Ok(())
}

fn enum_value(value: &str, options: &[&str], path: &Path) -> io::Result<u32> {
    options
        .iter()
        .position(|&item| item == value.trim())
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("unknown ObjectFilter value {value:?}")))
}
