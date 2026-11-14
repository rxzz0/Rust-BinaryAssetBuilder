use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, percentage, significant_children,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const KIND: u32 = 0x08e2_5591;
const REAL: u32 = 0x6458_64f3;
const UINT: u32 = 0x7631_77a4;
const BOOL: u32 = 0x77e3_f4f7;
const UPGRADE: u32 = 0x4aba_1f29;
const FX: u32 = 0x0d1c_8e83;
const CATEGORY: u32 = 0x45f1_59e8;
const DISABLED: u32 = 0x1965_70c9;
const MODIFIER: u32 = 0x09dd_cff3;
const FIELDS: &[(&str, u32, usize)] = &[
    ("EffectRadius", REAL, 0x08),
    ("MaxBeneficiaries", UINT, 0x0c),
    ("HealingPercentPerSecond", REAL, 0x10),
    ("HealingPointsPerSecond", REAL, 0x14),
    ("SurveyDelayFrames", UINT, 0x18),
    ("UpgradeRequired", UPGRADE, 0x1c),
    ("NonStackable", BOOL, 0x44),
    ("AntiCategoryMask", CATEGORY, 0x20),
    ("AntiFX", FX, 0x24),
    ("HealFX", FX, 0x28),
    ("AffectAttached", BOOL, 0x45),
    ("AffectWoundedOnly", BOOL, 0x46),
    ("AffectUnderAttack", BOOL, 0x47),
    ("AffectContained", BOOL, 0x48),
    ("DisabledTypesToProcess", DISABLED, 0x2c),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = FIELDS
        .iter()
        .map(|&(name, _, _)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x4c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for &(name, token, offset) in FIELDS {
        let value = node.attribute(name);
        let word = match token {
            BOOL => {
                data.payload[start + offset] = boolean(node, name, false, path)?;
                continue;
            }
            REAL if offset == 0x10 => percentage(value.unwrap_or("0"), name, path)?.to_bits(),
            REAL => finite_number(value.unwrap_or("0"), name, path)?.to_bits(),
            UINT => value
                .unwrap_or(if offset == 0x18 { "20" } else { "0" })
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?,
            UPGRADE | FX => {
                let kind = if token == UPGRADE {
                    "UpgradeTemplate"
                } else {
                    "FXList"
                };
                reference(value.unwrap_or(""), kind, path, references, reference_types)?
            }
            CATEGORY => category_flags(value.unwrap_or(""), name, path)?,
            DISABLED => {
                let mut bits = 0;
                for flag in value.unwrap_or("").split_ascii_whitespace() {
                    bits |= 1
                        << super::super::global_ui::enum_value(
                            flag,
                            super::production::DISABLED,
                            path,
                        )?;
                }
                bits
            }
            _ => unreachable!(),
        };
        data.word(start + offset, word);
    }
    entries.extend(fields(start, FIELDS));

    let mut children = significant_children(node).peekable();
    let mut modifiers = Vec::new();
    while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Modifier"))) {
        validate_attributes(child, &[], path)?;
        if child.children().any(|node| node.is_element()) {
            return Err(invalid(path, "Modifier requires reference text"));
        }
        let value: String = child
            .children()
            .filter(Node::is_text)
            .filter_map(|node| node.text())
            .collect();
        modifiers.push(reference(
            &value,
            "AttributeModifier",
            path,
            references,
            reference_types,
        )?);
    }
    let count = start + 0x30;
    let first = data.list(count, modifiers.len(), 4)?;
    let items: Vec<_> = modifiers
        .into_iter()
        .enumerate()
        .map(|(index, ordinal)| {
            let offset = first + index * 4;
            data.word(offset, ordinal);
            (0, Target::Data(offset))
        })
        .collect();
    entries.push(Entry::named(
        "Modifier",
        MODIFIER,
        Target::Table(Table::array(count, items)),
    ));
    for (name, offset) in [("AllowFilter", 0x38), ("ContainerFilter", 0x3c)] {
        let target =
            if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                let first = data.allocate(start + offset, 0x70)?;
                Target::Table(super::parking::encode_inline_filter(
                    child,
                    first,
                    data,
                    reference_types,
                )?)
            } else {
                Target::Null
            };
        entries.push(Entry::named(name, 0xe46e_a3b2, target));
    }
    let name = "ObjectStatusValidation";
    let target = if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name)))
    {
        Target::Table(super::secondary_damage::encode_status(
            child,
            start + 0x40,
            data,
        )?)
    } else {
        Target::Null
    };
    entries.push(Entry::named(name, 0x3e8f_938c, target));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected PassiveAreaEffectBehavior child"));
    }
    Ok((KIND, Table::record(entries)))
}

pub(super) fn category_flags(value: &str, name: &str, path: &std::path::Path) -> io::Result<u32> {
    static CATEGORIES: OnceLock<Vec<String>> = OnceLock::new();
    let values = CATEGORIES.get_or_init(|| {
        super::ai_update::enum_values(
            include_str!("../../../../schemas/ra3/xsd/AssetTypeAttributeModifier.xsd"),
            "AttributeModifierCategoryType",
        )
    });
    value.split_ascii_whitespace().try_fold(0, |bits, flag| {
        let index = values
            .iter()
            .position(|candidate| candidate == flag)
            .ok_or_else(|| invalid(path, format!("invalid {name} {flag}")))?;
        Ok(bits | (1 << index))
    })
}

pub(super) fn reference(
    value: &str,
    kind: &str,
    path: &std::path::Path,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<u32> {
    if value.trim().is_empty() {
        return Ok(0);
    }
    let plain = value.split_once('\\').map_or(value, |(name, _)| name);
    let id = asset_reference(plain, kind, path)?;
    references.push(id.into());
    reference_types.push(id.type_id);
    Ok(references.len() as u32)
}
