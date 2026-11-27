use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, percentage, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, module_base};
use super::draws::{boolean, fields};
use super::passive_area::reference;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const REFUND: u32 = 0xe1bd_0410;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KEEP: u32 = 0xd014_a1c2;
    const UPGRADE: u32 = 0x510d_79c7;
    const UPGRADE_REF: u32 = 0x4aba_1f29;

    let path = data.path;
    let name = node.tag_name().name();
    let (kind, size, allowed): (_, _, &[_]) = match name {
        "KeepObjectDie" => (KEEP, 0x3c, &["id", "CollapsingTime", "StayOnRadar"]),
        "UpgradeDie" => (UPGRADE, 0x38, &["id", "UpgradeId"]),
        "RefundDie" => (
            REFUND,
            0x48,
            &["id", "UpgradeRequired", "BuildingRequired", "RefundPercent"],
        ),
        _ => return Err(invalid(path, format!("unexpected death module {name}"))),
    };
    validate_attributes(node, allowed, path)?;
    let mut children = significant_children(node).peekable();
    let mux = children
        .next_if(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
        .ok_or_else(|| invalid(path, format!("{name} requires DieMuxData")))?;
    let start = data.allocate(pointer, size)?;

    let mut entries = if kind == KEEP {
        let time = node.attribute("CollapsingTime").unwrap_or("0s");
        data.word(
            start + 0x34,
            sage_time(time, "CollapsingTime", path)?.to_bits(),
        );
        data.payload[start + 0x38] = boolean(node, "StayOnRadar", false, path)?;
        fields(
            start,
            &[
                ("CollapsingTime", 0x9986_69e8, 0x34),
                ("StayOnRadar", 0x77e3_f4f7, 0x38),
            ],
        )
        .collect()
    } else {
        let field = if kind == UPGRADE {
            "UpgradeId"
        } else {
            "UpgradeRequired"
        };
        let ordinal = reference(
            node.attribute(field).unwrap_or(""),
            "UpgradeTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(start + 0x34, ordinal);
        vec![Entry::named(field, UPGRADE_REF, Target::Data(start + 0x34))]
    };
    if kind == REFUND {
        let names = node
            .attribute("BuildingRequired")
            .unwrap_or("")
            .split_ascii_whitespace();
        let count = start + 0x38;
        let strings = data.list(count, names.clone().count(), 8)?;
        let mut items = Vec::new();
        for (index, name) in names.enumerate() {
            let offset = strings + index * 8;
            data.string(offset, name)?;
            items.push((0, Target::Data(offset)));
        }
        let refund = node.attribute("RefundPercent").unwrap_or("0");
        data.word(
            start + 0x40,
            percentage(refund, "RefundPercent", path)?.to_bits(),
        );
        let filter = match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter"))) {
            Some(filter) => Target::Table(super::parking::encode_filter(
                filter,
                start + 0x44,
                data,
                reference_types,
            )?),
            None => Target::Null,
        };
        entries.extend([
            Entry::named(
                "BuildingRequired",
                0x6d66_ae18,
                Target::Table(Table::array(count, items)),
            ),
            Entry::named("RefundPercent", 0x6458_64f3, Target::Data(start + 0x40)),
            Entry::named("ObjectFilter", 0xe46e_a3b2, filter),
        ]);
    }
    if children.next().is_some() {
        return Err(invalid(path, format!("unexpected {name} child")));
    }

    let behavior = module_base(node, kind, start, data, false)?;
    let mux = encode_die_mux(mux, start + 8, data)?;
    entries.insert(
        0,
        base(
            0x9e4f_2b72,
            Table::record(vec![
                behavior,
                Entry::named("DieMuxData", 0x0084_001b, Target::Table(mux)),
            ]),
        ),
    );
    Ok((kind, Table::record(entries)))
}
