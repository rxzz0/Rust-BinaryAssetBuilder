use super::ai_heuristics::write_heuristics;
use super::binary::BinaryData;
use super::global_ui::{filter_reference_types, object_filter};
use super::{
    Marshaled, asset_reference, finite_number, percentage, required_element, sage_time,
    significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;
use std::path::Path;

const ENTRY_SIZE: usize = 56;
const FILTER: &str = "BlockIfThingNearFactory";
const BUILD_HEURISTICS: [&str; 18] = [
    "CONSTANT",
    "BEST_UNIT_EXPECTED",
    "BEST_UNIT_ACTUAL",
    "BEST_UNIT_WEIGHTED",
    "FEWEST_UNIT",
    "RANDOM_UNIT",
    "WORST_UNIT",
    "NEEDED_UNIT",
    "FACTORY",
    "TECH",
    "POWER",
    "REFINERY",
    "HARVESTER",
    "UNIQUE",
    "UPGRADE",
    "UPGRADE_TECH",
    "BUILD_TEST",
    "UPGRADE_TEST",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let heuristic = required_element(&mut children, "Heuristic", path)?;
    let entries: Vec<_> = children.collect();
    if entries.is_empty() {
        return Err(invalid(path, "expected at least one AI build entry"));
    }
    let mut binary = BinaryData {
        payload: vec![0; 20],
        relocations: Vec::new(),
        path,
    };
    let mut entry_refs = Vec::new();
    let first = binary.list(12, entries.len(), ENTRY_SIZE)?;
    let entries_end = binary.payload.len();
    for (index, entry) in entries.into_iter().enumerate() {
        write_entry(
            entry,
            first + index * ENTRY_SIZE,
            &mut binary,
            &mut entry_refs,
        )?;
    }
    let heuristic_start = binary.payload.len();
    let mut reference_types = write_heuristics(heuristic, 4, &mut binary)?;
    reference_types.extend(entry_refs);
    // The SDK records the two root pointers before pointers inside the entry
    // array, the entry filters, and finally the heuristic array.
    binary
        .relocations
        .sort_by_key(|&offset| match offset as usize {
            16 => 0,
            8 => 1,
            offset if offset < entries_end => 2,
            offset if offset < heuristic_start => 3,
            _ => 4,
        });
    binary.relocations.push(u32::MAX);
    Ok(Marshaled {
        payload: binary.payload,
        relocations: binary.relocations,
        reference_types,
        ..Marshaled::default()
    })
}

fn write_entry(
    node: Node<'_, '_>,
    offset: usize,
    binary: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<()> {
    let path = binary.path;
    if !node.has_tag_name((SAGE_NS, "BuildEntry")) {
        return Err(invalid(path, "expected AI BuildEntry"));
    }
    validate_attributes(
        node,
        &[
            "Build",
            "Upgrade",
            "Heuristic",
            "PercentLow",
            "PercentHigh",
            "ExpectedEfficiencyWeight",
            "ActualEfficiencyWeight",
            "SampleSizeForFullWeight",
            "MinTime",
            "ForceTime",
            "BlockIfThingNearFactoryDistance",
            "CountModifier",
            "Score",
        ],
        path,
    )?;

    for attr in node.attributes() {
        let (relative, kind) = match attr.name() {
            "Build" => (0, "GameObject"),
            "Upgrade" => (4, "UpgradeTemplate"),
            _ => continue,
        };
        if !attr.value().trim().is_empty() {
            let id = asset_reference(attr.value(), kind, path)?;
            binary.word(offset + relative, id.instance_id);
            reference_types.push(id.type_id);
        }
    }
    let field = |name, default| node.attribute(name).unwrap_or(default).trim();
    let heuristic = field("Heuristic", "CONSTANT");
    let heuristic = BUILD_HEURISTICS
        .iter()
        .position(|&name| name == heuristic)
        .ok_or_else(|| invalid(path, format!("unknown AI build heuristic {heuristic:?}")))?;
    let sample_size = field("SampleSizeForFullWeight", "10000")
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid AI build entry SampleSizeForFullWeight"))?;
    let words = [
        heuristic as u32,
        percentage(field("PercentLow", "0%"), "PercentLow", path)?.to_bits(),
        percentage(field("PercentHigh", "0%"), "PercentHigh", path)?.to_bits(),
        finite_number(
            field("ExpectedEfficiencyWeight", "0.4"),
            "ExpectedEfficiencyWeight",
            path,
        )?
        .to_bits(),
        finite_number(
            field("ActualEfficiencyWeight", "0.6"),
            "ActualEfficiencyWeight",
            path,
        )?
        .to_bits(),
        sample_size as u32,
        sage_time(field("MinTime", "0s"), "MinTime", path)?.to_bits(),
        sage_time(field("ForceTime", "99999s"), "ForceTime", path)?.to_bits(),
        finite_number(
            field("BlockIfThingNearFactoryDistance", "150.0"),
            "BlockIfThingNearFactoryDistance",
            path,
        )?
        .to_bits(),
        finite_number(field("CountModifier", "1.0"), "CountModifier", path)?.to_bits(),
        finite_number(field("Score", "0.0"), "Score", path)?.to_bits(),
    ];
    for (index, word) in words.into_iter().enumerate() {
        binary.word(offset + 8 + 4 * index, word);
    }

    let mut children = significant_children(node);
    if let Some(filter) = children.next() {
        if !filter.has_tag_name((SAGE_NS, FILTER)) || children.next().is_some() {
            return Err(invalid(path, "unexpected content in AI BuildEntry"));
        }
        let start = binary.allocate(offset + 52, 112)?;
        object_filter(filter, start, binary)?;
        reference_types.extend(filter_reference_types(filter));
    }
    Ok(())
}
