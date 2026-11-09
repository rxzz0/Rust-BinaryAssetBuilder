use super::binary::BinaryData;
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
    let mut binary = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    if let Some(default) = node.attribute("DefaultHealthBar") {
        binary.word(4, import_health_bar(&mut data, default, 1, path)?);
    }
    let first = binary.list(8, mappings.len(), 4 * MAPPING_WORDS)?;

    for (index, mapping) in mappings.into_iter().enumerate() {
        if !mapping.has_tag_name((SAGE_NS, "ObjectMapping")) {
            return Err(invalid(path, "expected a HealthBarSettings ObjectMapping"));
        }
        validate_attributes(mapping, &["HealthBar"], path)?;
        let start = first + index * 4 * MAPPING_WORDS;
        binary.word(
            start,
            import_health_bar(
                &mut data,
                required(mapping, "HealthBar", path)?,
                start / 4,
                path,
            )?,
        );
        let mut children = significant_children(mapping);
        let filter = required_element(&mut children, "ObjectFilter", path)?;
        if children.next().is_some() {
            return Err(invalid(path, "unexpected content in ObjectMapping"));
        }
        object_filter(filter, start + 4, &mut binary)?;
        data.reference_types.extend(filter_reference_types(filter));
    }
    if !binary.relocations.is_empty() {
        binary.relocations.push(u32::MAX);
    }
    data.relocations = binary.relocations;
    if !data.imports.is_empty() {
        data.imports.push(u32::MAX);
    }
    data.payload = binary.payload;
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

pub(super) fn object_filter(
    node: Node<'_, '_>,
    start: usize,
    binary: &mut BinaryData<'_>,
) -> io::Result<()> {
    let path = binary.path;
    validate_attributes(
        node,
        &["Rule", "Relationship", "Alignment", "Include", "Exclude"],
        path,
    )?;
    let mut words = [0; 22];
    words[1] = enum_value(
        node.attribute("Rule").unwrap_or("NONE"),
        &["UNPARSED", "ALL", "ANY", "NONE"],
        path,
    )?;
    words[3] = enum_value(
        node.attribute("Alignment").unwrap_or("NONE"),
        &["NONE", "EVIL", "GOOD"],
        path,
    )?;
    for value in node
        .attribute("Relationship")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        words[2] |= 1
            << enum_value(
                value,
                &["ALLIES", "ENEMIES", "NEUTRAL", "SAME_PLAYER"],
                path,
            )?;
    }
    let schema = Document::parse(include_str!("../../../schemas/ra3/xsd/Includes/KindOf.xsd"))
        .expect("bundled KindOf schema must be valid");
    let kind_of: Vec<_> = schema
        .descendants()
        .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "enumeration")))
        .filter_map(|node| node.attribute("value"))
        .collect();
    for (field, base) in [("Include", 4), ("Exclude", 13)] {
        for value in node.attribute(field).unwrap_or("").split_ascii_whitespace() {
            let bit = enum_value(value, &kind_of, path)? as usize;
            words[base + bit / 32] |= 1 << (bit % 32);
        }
    }
    for (index, word) in words.into_iter().enumerate() {
        binary.word(start + 4 * index, word);
    }

    let mut children = significant_children(node).peekable();
    for (tag, offset) in [("IncludeThing", 96), ("ExcludeThing", 104)] {
        let mut ids = Vec::new();
        while let Some(item) = children.next_if(|item| item.has_tag_name((SAGE_NS, tag))) {
            let asset = asset_reference(text_value(item, path)?, "GameObject", path)?;
            ids.push(asset.instance_id);
        }
        let first = binary.list(start + offset, ids.len(), 4)?;
        for (index, id) in ids.into_iter().enumerate() {
            binary.word(first + 4 * index, id);
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in ObjectFilter"));
    }
    Ok(())
}

pub(super) fn filter_reference_types(node: Node<'_, '_>) -> impl Iterator<Item = u32> {
    std::iter::repeat_n(fast_hash(b"GameObject"), significant_children(node).count())
}

fn enum_value(value: &str, options: &[&str], path: &Path) -> io::Result<u32> {
    options
        .iter()
        .position(|&item| item == value.trim())
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("unknown ObjectFilter value {value:?}")))
}
