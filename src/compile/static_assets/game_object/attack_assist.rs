use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, hash_name, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{module_base, weapon_slot};
use super::draws::{boolean, fields};
use super::parking::encode_filter;
use super::secondary_damage::write_status_flags;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const NOTIFY: u32 = 0x02fd_f9e4;
const RESPOND: u32 = 0xec30_8e0e;
const REAL: u32 = 0x6458_64f3;
const UINT: u32 = 0x7631_77a4;
const INT: u32 = 0x23e6_50b8;
const BOOL: u32 = 0x77e3_f4f7;
const SLOT: u32 = 0x1848_c549;
const NOTIFY_FIELDS: &[(&str, u32, usize)] = &[
    ("NotifyRadius", REAL, 8),
    ("AssistResponseModuleId", 0xbdf0_7965, 0xc),
    ("SleepTime", 0x9986_69e8, 0x10),
    ("HasMobileFriends", BOOL, 0x24),
    ("IgnoreDisabledStatus", BOOL, 0x25),
    ("MaxFriendsToNotify", INT, 0x14),
    ("WeaponSlotID", UINT, 0x18),
    ("WeaponSlotType", SLOT, 0x1c),
];
const RESPONSE_FIELDS: &[(&str, u32, usize)] = &[
    ("ObjectStatus", 0xa7bd_811e, 8),
    ("WeaponSlotID", UINT, 0x24),
    ("WeaponSlotType", SLOT, 0x28),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let notify = node.has_tag_name((SAGE_NS, "NotifyWhenAttackingBehavior"));
    let (kind, size, descriptors, default_slot) = if notify {
        (NOTIFY, 0x28, NOTIFY_FIELDS, "1")
    } else {
        (RESPOND, 0x2c, RESPONSE_FIELDS, "0")
    };
    let allowed: Vec<_> = std::iter::once("id")
        .chain(descriptors.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    let mut entries = vec![module_base(node, kind, start, data, notify)?];
    for &(name, _, offset) in descriptors {
        let value = node.attribute(name);
        let word = match name {
            "NotifyRadius" => finite_number(value.unwrap_or("0"), name, path)?.to_bits(),
            "AssistResponseModuleId" => value.map_or(0, hash_name),
            "SleepTime" => sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
            "HasMobileFriends" | "IgnoreDisabledStatus" => {
                data.payload[start + offset] = boolean(node, name, false, path)?;
                continue;
            }
            "MaxFriendsToNotify" => value
                .unwrap_or("100")
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, "invalid MaxFriendsToNotify"))?
                as u32,
            "WeaponSlotID" => value
                .unwrap_or(default_slot)
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid(path, "invalid WeaponSlotID"))?,
            "WeaponSlotType" => weapon_slot(value.unwrap_or("PRIMARY_WEAPON"), path)?,
            "ObjectStatus" => {
                write_status_flags(value.unwrap_or(""), start + offset, data)?;
                continue;
            }
            _ => unreachable!("attack assist field {name}"),
        };
        data.word(start + offset, word);
    }
    entries.extend(fields(start, descriptors));
    if notify {
        let mut children = significant_children(node).peekable();
        let filter =
            match children.next_if(|child| child.has_tag_name((SAGE_NS, "AcceptableFriends"))) {
                Some(child) => {
                    Target::Table(encode_filter(child, start + 0x20, data, reference_types)?)
                }
                None => Target::Null,
            };
        if children.next().is_some() {
            return Err(invalid(
                path,
                "unexpected NotifyWhenAttackingBehavior child",
            ));
        }
        entries.push(Entry::named("AcceptableFriends", 0xe46e_a3b2, filter));
    } else if has_content(node) {
        return Err(invalid(
            path,
            "unexpected AttackAssistResponseBehavior content",
        ));
    }
    Ok((kind, Table::record(entries)))
}
