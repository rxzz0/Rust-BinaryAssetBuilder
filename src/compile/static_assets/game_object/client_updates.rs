use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, fast_hash, significant_children, validate_attributes};
use super::behaviors_basic::{base, module_base};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, has_content, invalid, required, windows_1252};
use roxmltree::Node;
use std::io;

const EVA: u32 = 0xc4ec_43b1;
const POWER: u32 = 0xaee8_3888;
const UINT: u32 = 0x7631_77a4;
const BOOL: u32 = 0x77e3_f4f7;
const COMMON: &[(&str, u32, usize)] = &[
    ("AnnouncementEventEnemy", EVA, 8),
    ("AnnouncementEventAlly", EVA, 12),
    ("AnnouncementEventOwner", EVA, 16),
];
const READY: &[(&str, u32, usize)] = &[
    ("SpecialPowerTemplate", POWER, 20),
    ("JustMonitorPercentReady", BOOL, 24),
    ("CheckPlayerPowerVisibility", BOOL, 25),
];
const CREATE: &[(&str, u32, usize)] = &[
    ("DelayBeforeAnnouncementLogicFrames", UINT, 20),
    ("OnlyIfVisible", BOOL, 24),
    ("CountAsFirstSightedAnnouncement", BOOL, 25),
    ("UseObjectsPosition", BOOL, 26),
    ("CreateFakeRadarEvent", BOOL, 27),
];

pub(super) fn encode(
    group: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    let path = data.path;
    let modules: Vec<_> = significant_children(group).collect();
    let count = data.allocate(0x1e0, 8)?;
    let pointers = data.list(count, modules.len(), 4)?;
    let mut items = Vec::with_capacity(modules.len());
    for (index, node) in modules.into_iter().enumerate() {
        let pointer = pointers + index * 4;
        if node.has_tag_name((SAGE_NS, "LargeGroupAudioClientUpdate")) {
            items.push((0, large_group_audio(node, pointer, data)?));
            continue;
        }
        let (kind, specific) = match (node.tag_name().namespace(), node.tag_name().name()) {
            (Some(SAGE_NS), "EvaAnnounceSpecialPowerReadyClientUpdate") => (0xa220_e7d6, READY),
            (Some(SAGE_NS), "EvaAnnounceClientCreate") => (0xd0ee_fddd, CREATE),
            (_, name) => return Err(invalid(path, format!("unsupported client update {name}"))),
        };
        let allowed: Vec<_> = COMMON
            .iter()
            .chain(specific)
            .map(|&(name, _, _)| name)
            .chain(["id"])
            .collect();
        validate_attributes(node, &allowed, path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected EVA client update content"));
        }
        let start = data.allocate(pointer, 0x1c)?;
        let parent = module_base(node, kind, start, data, false)?;
        let mut entries = vec![base(0x4cad_6edc, Table::record(vec![parent]))];
        if kind == 0xa220_e7d6 {
            required(node, "SpecialPowerTemplate", path)?;
        }
        for attr in node.attributes() {
            if let Some(&(_, token, offset)) = COMMON
                .iter()
                .chain(specific)
                .find(|&&(name, token, _)| matches!(token, EVA | POWER) && name == attr.name())
            {
                let target_type = if token == EVA {
                    "EvaEvent"
                } else {
                    "SpecialPowerTemplate"
                };
                let ordinal = super::passive_area::reference(
                    attr.value(),
                    target_type,
                    path,
                    references,
                    reference_types,
                )?;
                data.word(start + offset, ordinal);
            }
        }
        for &(name, token, offset) in specific {
            match token {
                BOOL => data.payload[start + offset] = boolean(node, name, false, path)?,
                UINT => {
                    let value = node
                        .attribute(name)
                        .unwrap_or("0")
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| invalid(path, format!("invalid {name}")))?;
                    data.word(start + offset, value);
                }
                _ => {}
            }
        }
        entries.extend(fields(start, COMMON));
        entries.extend(fields(start, specific));
        items.push((0, update_module(kind, entries)));
    }
    Ok(Target::Table(Table::array(count, items)))
}

fn update_module(kind: u32, entries: Vec<Entry>) -> Target {
    Target::Table(Table::sequence(vec![Entry::new(
        2,
        kind,
        Target::Table(Table::record(entries)),
    )]))
}

fn large_group_audio(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Target> {
    const KIND: u32 = 0x9826_a297;
    const TIMINGS: [(&str, &str, usize); 2] = [
        ("FramesBetweenUpdatesMin", "3", 8),
        ("FramesBetweenUpdatesVariation", "2", 12),
    ];
    let path = data.path;
    let allowed = ["id", TIMINGS[0].0, TIMINGS[1].0, "UnitWeight"];
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x1c)?;
    let parent = module_base(node, KIND, start, data, false)?;
    let mut entries = vec![base(0x4cad_6edc, Table::record(vec![parent]))];
    for (name, default, offset) in TIMINGS {
        let value = node
            .attribute(name)
            .unwrap_or(default)
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, format!("invalid {name}")))?;
        data.word(start + offset, value);
        entries.push(Entry::named(name, UINT, Target::Data(start + offset)));
    }
    let weight = node
        .attribute("UnitWeight")
        .unwrap_or("1")
        .trim()
        .parse::<u16>()
        .map_err(|_| invalid(path, "invalid UnitWeight"))?;
    data.payload[start + 0x18..start + 0x1a].copy_from_slice(&weight.to_le_bytes());
    entries.push(Entry::named(
        "UnitWeight",
        0x4350_1092,
        Target::Data(start + 0x18),
    ));
    let keys: Vec<_> = significant_children(node).collect();
    if keys.is_empty() {
        return Err(invalid(path, "LargeGroupAudioClientUpdate requires Key"));
    }
    let first = data.list(start + 0x10, keys.len(), 4)?;
    let mut items = Vec::with_capacity(keys.len());
    for (index, key) in keys.into_iter().enumerate() {
        if !key.has_tag_name((SAGE_NS, "Key")) {
            return Err(invalid(path, "expected LargeGroupAudioClientUpdate Key"));
        }
        validate_attributes(key, &[], path)?;
        let mut content = significant_children(key);
        let text = content
            .next()
            .filter(Node::is_text)
            .and_then(|text| text.text());
        let bytes = text
            .and_then(windows_1252)
            .filter(|_| content.next().is_none())
            .ok_or_else(|| invalid(path, "expected a Windows-1252 audio key"))?;
        let offset = first + index * 4;
        data.word(offset, fast_hash(&bytes));
        items.push((0, Target::Data(offset)));
    }
    entries.push(Entry::named(
        "Key",
        0x58be_c727,
        Target::Table(Table::array(start + 0x10, items)),
    ));
    Ok(update_module(KIND, entries))
}
