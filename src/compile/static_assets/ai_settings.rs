use super::ai_personality::write_base_criteria;
use super::ai_strategic::enum_index;
use super::binary::BinaryData;
use super::global_ui::{filter_reference_types, object_filter};
use super::{
    Marshaled, finite_number, sage_time, schema_enums, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const DATA_SCHEMA: &str = include_str!("../../../schemas/ra3/xsd/AssetTypeAIData.xsd");
pub(super) const MICRO_SCHEMA: &str =
    include_str!("../../../schemas/ra3/xsd/SkirmishAI/AssetTypeAIMicroManager.xsd");
pub(super) const POWER_DELAY: &str = "SpecificSpecialPowerDelay";
const XSD: &str = "http://www.w3.org/2001/XMLSchema";

pub(super) fn data(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    if has_content(node) {
        return Err(invalid(path, "unexpected content in AIData"));
    }
    let mut binary = BinaryData {
        payload: vec![0; 220],
        relocations: Vec::new(),
        path,
    };
    write_attributes(node, DATA_SCHEMA, 204, Some(216), &mut binary)?;
    binary.relocations.push(u32::MAX);
    Ok(Marshaled {
        payload: binary.payload,
        relocations: binary.relocations,
        ..Marshaled::default()
    })
}

pub(super) fn micro(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let mut binary = BinaryData {
        payload: vec![0; 176],
        relocations: Vec::new(),
        path,
    };
    write_attributes(node, MICRO_SCHEMA, 160, None, &mut binary)?;
    let mut children = significant_children(node).peekable();
    let mut delays = Vec::new();
    while let Some(delay) = children.next_if(|child| child.has_tag_name((SAGE_NS, POWER_DELAY))) {
        delays.push(delay);
    }
    let first = binary.list(144, delays.len(), 16)?;
    for (index, delay) in delays.into_iter().enumerate() {
        write_power_delay(delay, first + 16 * index, &mut binary)?;
    }
    if let Some(criteria) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "RetreatBaseCriteria")))
    {
        write_base_criteria(criteria, 152, &mut binary)?;
    }
    let mut reference_types = Vec::new();
    if let Some(filter) = children.next_if(|child| child.has_tag_name((SAGE_NS, "IgnoreTargets"))) {
        let start = binary.allocate(156, 112)?;
        object_filter(filter, start, &mut binary)?;
        reference_types.extend(filter_reference_types(filter));
    }
    if children.next().is_some() {
        return Err(invalid(path, "unsupported content in AIMicroManagerData"));
    }
    if !binary.relocations.is_empty() {
        binary.relocations.push(u32::MAX);
    }
    Ok(Marshaled {
        payload: binary.payload,
        relocations: binary.relocations,
        reference_types,
        ..Marshaled::default()
    })
}

fn write_power_delay(
    node: Node<'_, '_>,
    offset: usize,
    binary: &mut BinaryData<'_>,
) -> io::Result<()> {
    const FIELDS: [&str; 4] = [
        "PowerType",
        "TogglePowerSubType",
        "MinimumDelay",
        "MaximumDelay",
    ];
    let path = binary.path;
    validate_attributes(node, &FIELDS, path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected content in SpecificSpecialPowerDelay",
        ));
    }
    static ENUMS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    let [powers, hints] = ENUMS.get_or_init(|| {
        schema_enums(
            super::ai_command::SCHEMA,
            ["AISpecialPowerHeuristic", "ToggleStatusAiHint"],
        )
    });
    let words = [
        enum_index(required(node, FIELDS[0], path)?, powers, FIELDS[0], path)?,
        enum_index(
            node.attribute(FIELDS[1]).unwrap_or("TOGGLE_NO_HINT"),
            hints,
            FIELDS[1],
            path,
        )?,
        sage_time(node.attribute(FIELDS[2]).unwrap_or("0s"), FIELDS[2], path)?.to_bits(),
        sage_time(node.attribute(FIELDS[3]).unwrap_or("0s"), FIELDS[3], path)?.to_bits(),
    ];
    for (index, word) in words.into_iter().enumerate() {
        binary.word(offset + 4 * index, word);
    }
    Ok(())
}

fn write_attributes(
    node: Node<'_, '_>,
    schema: &str,
    flag_start: usize,
    byte_offset: Option<usize>,
    binary: &mut BinaryData<'_>,
) -> io::Result<()> {
    let schema = Document::parse(schema).expect("bundled AI settings schema must be valid");
    let kind = node.tag_name().name();
    let definition = schema
        .root_element()
        .children()
        .find(|item| {
            item.has_tag_name((XSD, "complexType")) && item.attribute("name") == Some(kind)
        })
        .expect("AI settings schema type must exist");
    let fields: Vec<_> = definition
        .descendants()
        .filter(|item| item.has_tag_name((XSD, "attribute")))
        .collect();
    let allowed: Vec<_> = std::iter::once("id")
        .chain(fields.iter().filter_map(|field| field.attribute("name")))
        .collect();
    let path = binary.path;
    validate_attributes(node, &allowed, path)?;

    // The Win32 layouts retain schema order within the word and boolean
    // fields, with booleans stored separately at the end of the fixed record.
    let mut word = 4;
    let mut flag = flag_start;
    for field in fields {
        let name = field.attribute("name").expect("named AI settings field");
        let value = match node.attribute(name).or_else(|| field.attribute("default")) {
            Some(value) => value,
            None => required(node, name, path)?,
        };
        let error = || invalid(path, format!("invalid {kind} {name}"));
        let encoded = match field.attribute("type") {
            Some("SageBool" | "xs:boolean") => {
                binary.payload[flag] = match value.trim() {
                    "true" | "1" => 1,
                    "false" | "0" => 0,
                    _ => return Err(error()),
                };
                flag += 1;
                continue;
            }
            Some("xs:unsignedByte") => {
                let offset = byte_offset.expect("AIData byte offset");
                binary.payload[offset] = value.trim().parse().map_err(|_| error())?;
                continue;
            }
            Some("AssetId") => {
                binary.string(word, value)?;
                word += 8;
                continue;
            }
            Some("SageReal" | "xs:float") => finite_number(value, name, path)?.to_bits(),
            Some("SageInt") => value.trim().parse::<i32>().map_err(|_| error())? as u32,
            Some("SageUnsignedInt") => value.trim().parse::<u32>().map_err(|_| error())?,
            Some("Time") => sage_time(value, name, path)?.to_bits(),
            _ => return Err(error()),
        };
        binary.word(word, encoded);
        word += 4;
    }
    Ok(())
}
