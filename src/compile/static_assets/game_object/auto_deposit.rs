use super::super::ai_strategic::{enum_index, flags};
use super::super::binary::BinaryData;
use super::super::fx_list::validate_leaf;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{sage_time, sage_uint, schema_enums, significant_children, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, sync::OnceLock};

const TRANSACTIONS: [&str; 3] = ["InitialCaptureBonus", "Deposit", "ResourceCap"];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x4d83_4921;
    const FIELDS: [(&str, u32, usize); 5] = [
        ("DepositInterval", 0x9986_69e8, 8),
        ("GiveNoXP", 0x77e3_f4f7, 0x2c),
        ("OnlyWhenGarrisoned", 0x77e3_f4f7, 0x2d),
        ("Flags", 0xe582_8841, 0xc),
        ("TransactionCategory", 0x9e55_f778, 0x10),
    ];
    const FLAGS: [&str; 2] = ["NONE", "ACTIVE_WHEN_REPAIRING"];
    static CATEGORY_VALUES: OnceLock<[Vec<String>; 1]> = OnceLock::new();

    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "DepositInterval",
            "GiveNoXP",
            "OnlyWhenGarrisoned",
            "Flags",
            "TransactionCategory",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x30)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let interval = sage_time(
        node.attribute("DepositInterval").unwrap_or("0.0s"),
        "DepositInterval",
        path,
    )?;
    data.word(start + 8, interval.to_bits());
    for &(name, _, offset) in &FIELDS[1..3] {
        data.payload[start + offset] = boolean(node, name, false, path)?;
    }
    data.word(
        start + 0xc,
        flags(
            node.attribute("Flags").unwrap_or("NONE"),
            &FLAGS,
            "Flags",
            path,
        )?,
    );
    let categories = CATEGORY_VALUES.get_or_init(|| {
        schema_enums(
            include_str!("../../../../schemas/ra3/xsd/Includes/MoneyTransactionCategory.xsd"),
            ["MoneyTransactionCategory"],
        )
    });
    let category = enum_index(
        node.attribute("TransactionCategory")
            .unwrap_or("AUTO_DEPOSIT"),
        &categories[0],
        "TransactionCategory",
        path,
    )?;
    data.word(start + 0x10, category);
    entries.extend(fields(start, &FIELDS));

    let mut groups = [Vec::new(), Vec::new(), Vec::new()];
    for child in significant_children(node) {
        let group = TRANSACTIONS
            .iter()
            .position(|&name| child.has_tag_name((SAGE_NS, name)))
            .ok_or_else(|| invalid(path, "unexpected AutoDepositUpdate child"))?;
        groups[group].push(child);
    }
    for (index, (name, nodes)) in TRANSACTIONS.into_iter().zip(groups).enumerate() {
        let count = start + 0x14 + index * 8;
        let first = data.list(count, nodes.len(), 8)?;
        let mut items = Vec::with_capacity(nodes.len());
        for (index, child) in nodes.into_iter().enumerate() {
            validate_leaf(child, &["Account", "Amount"], path)?;
            let offset = first + index * 8;
            let transaction = [("Account", offset), ("Amount", offset + 4)]
                .into_iter()
                .map(|(field, offset)| {
                    let value = sage_uint(required(child, field, path)?, field, path)?;
                    data.word(offset, value);
                    Ok(Entry::named(field, 0x7631_77a4, Target::Data(offset)))
                })
                .collect::<io::Result<_>>()?;
            items.push((0, Target::Table(Table::record(transaction))));
        }
        entries.push(Entry::named(
            name,
            0xb706_334d,
            Target::Table(Table::array(count, items)),
        ));
    }
    Ok((KIND, Table::record(entries)))
}
