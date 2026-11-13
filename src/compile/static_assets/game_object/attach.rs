use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{death_flags, module_base};
use super::draws::write_model_conditions;
use super::secondary_damage::{STATUS_BYTES, write_status_flags};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const KIND: u32 = 0xe9fd_14c9;
const STATUS: u32 = 0xa7bd_811e;
const MODEL: u32 = 0x67db_bfc9;
const REAL: u32 = 0x6458_64f3;
const TIME: u32 = 0x9986_69e8;
const EVA: u32 = 0xc4ec_43b1;
const FX: u32 = 0x0d1c_8e83;
const FLAGS: u32 = 0x48cd_8925;
const DEATH: u32 = 0xc608_ed36;
const DAMAGE_MASK: u32 = 0x07fa_aea4;
const DEATH_MASK: u32 = 0x034d_f1a1;
const FIELDS: &[(&str, u32, usize, &str)] = &[
    ("ParentStatus", STATUS, 8, ""),
    ("ParentStatusAttached", STATUS, 0x24, ""),
    ("AttachedObjectStatus", STATUS, 0x40, ""),
    ("AttachedModelConditions", MODEL, 0x5c, ""),
    ("ForbiddenParentStatus", STATUS, 0x98, ""),
    ("IgnoreForbiddenParentStatusStatus", STATUS, 0xb4, ""),
    ("ParentStatusToCopy", STATUS, 0xd0, ""),
    ("ParentStatusToPrefer", STATUS, 0xd4, ""),
    ("Range", REAL, 0xd8, "0"),
    ("CloseEnoughRange", REAL, 0xdc, "1000"),
    ("ParentOwnerAttachmentEvaEvent", EVA, 0xe0, ""),
    ("ParentAllyAttachmentEvaEvent", EVA, 0xe4, ""),
    ("ParentEnemyAttachmentEvaEvent", EVA, 0xe8, ""),
    ("AttachFXList", FX, 0xec, ""),
    ("DetachFXList", FX, 0xf0, ""),
    ("ParentOwnerDiedEvaEvent", EVA, 0xf4, ""),
    ("ParentAllyDiedEvaEvent", EVA, 0xf8, ""),
    ("ParentEnemyDiedEvaEvent", EVA, 0xfc, ""),
    ("InitialAttachDelay", TIME, 0x100, "0s"),
    ("IdleScanDelay", TIME, 0x104, "0.35s"),
    (
        "Flags",
        FLAGS,
        0x108,
        "STICK_TO_PARENT TELEPORT USE_GEOMETRY",
    ),
    ("ParentDeathTypeToListenFor", DEATH, 0x10c, "ALL"),
    ("NoDieIfStatusActive", STATUS, 0x110, ""),
    ("ParentPositionElevation", REAL, 0x12c, "0"),
    ("BounceAmount", REAL, 0x130, "0.2"),
    ("BounceTimeout", TIME, 0x134, "0.33s"),
    ("DamageTypesToNotLeech", DAMAGE_MASK, 0x138, ""),
    ("DeathTypesToNotLeech", DEATH_MASK, 0x13c, ""),
    ("ForbiddenParentStatusDieDeathType", DEATH, 0x140, "NORMAL"),
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
        .map(|&(name, _, _, _)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x150)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let mut ref_words = [0; 8];
    for attr in node.attributes() {
        if let Some(&(_, token, relative, _)) = FIELDS
            .iter()
            .find(|&&(name, token, _, _)| matches!(token, EVA | FX) && name == attr.name())
        {
            let kind = if token == EVA { "EvaEvent" } else { "FXList" };
            ref_words[(relative - 0xe0) / 4] = super::passive_area::reference(
                attr.value(),
                kind,
                path,
                references,
                reference_types,
            )?;
        }
    }
    for &(name, token, relative, default) in FIELDS {
        let offset = start + relative;
        let value = node.attribute(name).unwrap_or(default);
        let target = match token {
            STATUS if matches!(relative, 0xd0 | 0xd4) => {
                if node.attribute(name).is_some() {
                    let first = data.allocate(offset, STATUS_BYTES)?;
                    write_status_flags(value, first, data)?;
                    Target::Data(first)
                } else {
                    Target::Null
                }
            }
            STATUS => {
                write_status_flags(value, offset, data)?;
                Target::Data(offset)
            }
            MODEL => {
                write_model_conditions(value, offset, data)?;
                Target::Data(offset)
            }
            DAMAGE_MASK | DEATH_MASK => {
                if node.attribute(name).is_some() {
                    let first = data.allocate(offset, 8)?;
                    let bits = if token == DAMAGE_MASK {
                        super::spawn::damage_flags(value, data)?
                    } else {
                        death_flags(value, path)?
                    };
                    data.payload[first..first + 8].copy_from_slice(&bits.to_le_bytes());
                    Target::Data(first)
                } else {
                    Target::Null
                }
            }
            _ => {
                let word = match token {
                    REAL => finite_number(value, name, path)?.to_bits(),
                    TIME => sage_time(value, name, path)?.to_bits(),
                    EVA | FX => ref_words[(relative - 0xe0) / 4],
                    FLAGS => {
                        static VALUES: OnceLock<Vec<String>> = OnceLock::new();
                        let values = VALUES.get_or_init(|| {
                            super::ai_update::enum_values(
                                include_str!(
                                    "../../../../schemas/ra3/xsd/Modules/AttachUpdate.xsd"
                                ),
                                "AttachUpdateFlagsType",
                            )
                        });
                        let mut flags = 0;
                        for flag in value.split_ascii_whitespace() {
                            flags |= 1 << enum_index(values, flag, name, path)?;
                        }
                        flags
                    }
                    DEATH => {
                        static VALUES: OnceLock<Vec<String>> = OnceLock::new();
                        let values = VALUES.get_or_init(|| {
                            super::ai_update::enum_values(
                                include_str!("../../../../schemas/ra3/xsd/Includes/Death.xsd"),
                                "DeathType",
                            )
                        });
                        enum_index(values, value, name, path)? as u32
                    }
                    _ => unreachable!(),
                };
                data.word(offset, word);
                Target::Data(offset)
            }
        };
        entries.push(Entry::named(name, token, target));
    }
    let mut children = significant_children(node).peekable();
    let filter = if let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")))
    {
        let first = data.allocate(start + 0x144, 0x70)?;
        Target::Table(super::parking::encode_inline_filter(
            child,
            first,
            data,
            reference_types,
        )?)
    } else {
        Target::Null
    };
    entries.push(Entry::named("ObjectFilter", 0xe46e_a3b2, filter));
    let name = "ModifierToLeechFromParent";
    let mut refs = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, name)) || child.children().any(|node| node.is_element()) {
            return Err(invalid(path, "unexpected AttachUpdate child"));
        }
        validate_attributes(child, &[], path)?;
        let value: String = child
            .children()
            .filter(Node::is_text)
            .filter_map(|node| node.text())
            .collect();
        refs.push(super::passive_area::reference(
            &value,
            "AttributeModifier",
            path,
            references,
            reference_types,
        )?);
    }
    let count = start + 0x148;
    let first = data.list(count, refs.len(), 4)?;
    let items: Vec<_> = refs
        .into_iter()
        .enumerate()
        .map(|(index, ordinal)| {
            let offset = first + index * 4;
            data.word(offset, ordinal);
            (0, Target::Data(offset))
        })
        .collect();
    entries.push(Entry::named(
        name,
        0x09dd_cff3,
        Target::Table(Table::array(count, items)),
    ));
    Ok((KIND, Table::record(entries)))
}

fn enum_index(
    values: &[String],
    value: &str,
    field: &str,
    path: &std::path::Path,
) -> io::Result<usize> {
    values
        .iter()
        .position(|name| name == value)
        .ok_or_else(|| invalid(path, format!("invalid {field} {value}")))
}
