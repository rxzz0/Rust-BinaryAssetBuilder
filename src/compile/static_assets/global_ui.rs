use super::binary::BinaryData;
use super::{
    Marshaled, asset_reference, encode_words, fast_hash, finite_number, required_element,
    significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;

const HEALTH_BAR: &str = "HealthBarTemplate";
const MAPPING_WORDS: usize = 29;

pub(super) fn radius_cursor(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let template = required_element(&mut children, "DecalTemplate", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected RadiusCursor child"));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    super::experience_level::radius_decal(template, 4, &mut data, &mut result)?;
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

pub(super) fn purchase_player_tech_button(
    node: Node<'_, '_>,
    path: &Path,
) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "PlayerTech", "StateData"], path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected PurchasePlayerTechButtonTemplate content",
        ));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for (field, kind, offset) in [
        ("PlayerTech", "PlayerTech", 4),
        ("StateData", "ButtonSingleStateData", 8),
    ] {
        write_import(
            required(node, field, path)?,
            kind,
            offset,
            &mut data,
            &mut result,
        )?;
    }
    Ok(data.finish(result))
}

pub(super) fn button_single_state(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let state = required_element(&mut children, "State", path)?;
    validate_attributes(
        state,
        &["Image", "Title", "Description", "TypeDescription"],
        path,
    )?;
    if children.next().is_some() || has_content(state) {
        return Err(invalid(path, "unexpected ButtonSingleStateData content"));
    }
    let mut data = BinaryData {
        payload: vec![0; 32],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(image) = state
        .attribute("Image")
        .filter(|value| !value.trim().is_empty())
    {
        write_import(image, "PackedTextureImage", 4, &mut data, &mut result)?;
        result.imports.push(u32::MAX);
    }
    for (name, offset) in [("Title", 8), ("Description", 16), ("TypeDescription", 24)] {
        if let Some(value) = state.attribute(name) {
            data.string(offset, value)?;
        }
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

pub(super) fn unit_type_icon(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Image"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(image) = node
        .attribute("Image")
        .filter(|value| !value.trim().is_empty())
    {
        write_import(image, "PackedTextureImage", 4, &mut data, &mut result)?;
    }
    let mut children = significant_children(node).peekable();
    if children.peek().is_some() {
        let offset = required_element(&mut children, "Offset", path)?;
        validate_attributes(offset, &["x", "y"], path)?;
        if has_content(offset) || children.next().is_some() {
            return Err(invalid(path, "unexpected content in UnitTypeIcon"));
        }
        let start = data.allocate(8, 8)?;
        for (axis, position) in [("x", start), ("y", start + 4)] {
            let value = finite_number(offset.attribute(axis).unwrap_or("0"), axis, path)?;
            data.word(position, value.to_bits());
        }
        data.relocations.push(u32::MAX);
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

pub(super) fn packed_texture_image(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Texture", "Rotated"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 36],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(texture) = node
        .attribute("Texture")
        .filter(|value| !value.trim().is_empty())
    {
        write_import(texture, "Texture", 4, &mut data, &mut result)?;
        result.imports.push(u32::MAX);
    }
    let mut children = significant_children(node);
    for (name, offset) in [("Dimensions", 8), ("Coords", 16), ("TextureDimensions", 24)] {
        let field = required_element(&mut children, name, path)?;
        validate_attributes(field, &["x", "y"], path)?;
        if has_content(field) {
            return Err(invalid(path, format!("unexpected content in {name}")));
        }
        for (axis, position) in [("x", offset), ("y", offset + 4)] {
            let value = field
                .attribute(axis)
                .unwrap_or("0")
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name} {axis}")))?;
            data.word(position, value as u32);
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in PackedTextureImage"));
    }
    let rotated = match node.attribute("Rotated").map(str::trim) {
        None | Some("false" | "0") => 0,
        Some("true" | "1") => 1,
        Some(value) => return Err(invalid(path, format!("invalid Rotated {value:?}"))),
    };
    data.word(32, rotated);
    result.payload = data.payload;
    Ok(result)
}

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
    for (field, base) in [("Include", 4), ("Exclude", 13)] {
        words[base..base + 9]
            .copy_from_slice(&kind_of_flags(node.attribute(field).unwrap_or(""), path)?);
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

pub(super) fn kind_of_flags(value: &str, path: &Path) -> io::Result<[u32; 9]> {
    use std::sync::OnceLock;
    static KINDS: OnceLock<Vec<String>> = OnceLock::new();
    let mut words = [0; 9];
    if value.trim().is_empty() {
        return Ok(words);
    }
    let kinds = KINDS.get_or_init(|| {
        let schema = Document::parse(include_str!("../../../schemas/ra3/xsd/Includes/KindOf.xsd"))
            .expect("bundled KindOf schema must be valid");
        schema
            .descendants()
            .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "enumeration")))
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    });
    for flag in value.split_ascii_whitespace() {
        let bit = kinds
            .iter()
            .position(|kind| kind == flag)
            .filter(|&bit| bit < 32 * words.len())
            .ok_or_else(|| invalid(path, format!("unknown KindOf flag {flag:?}")))?;
        words[bit / 32] |= 1 << (bit % 32);
    }
    Ok(words)
}

pub(super) fn filter_reference_types(node: Node<'_, '_>) -> impl Iterator<Item = u32> {
    std::iter::repeat_n(fast_hash(b"GameObject"), significant_children(node).count())
}

pub(super) fn enum_value(value: &str, options: &[&str], path: &Path) -> io::Result<u32> {
    options
        .iter()
        .position(|&item| item == value.trim())
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("unknown ObjectFilter value {value:?}")))
}
