use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, sage_int, sage_uint, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::{boolean, fields};
use super::passive_area::reference;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x1e95_ee57;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "GrantMoney",
            "GrantMoneyOneTime",
            "SlavedCaptureObject",
            "UnificationType",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x20)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    data.word(
        start + 8,
        sage_uint(
            node.attribute("GrantMoney").unwrap_or("0"),
            "GrantMoney",
            path,
        )?,
    );
    if let Some(value) = node
        .attribute("SlavedCaptureObject")
        .filter(|value| !value.trim().is_empty())
    {
        let id = asset_reference(value, "GameObject", path)?;
        data.word(start + 0xc, id.instance_id);
        reference_types.push(id.type_id);
    }
    data.payload[start + 0x10] = boolean(node, "GrantMoneyOneTime", false, path)?;
    data.word(
        start + 0x14,
        enum_value(
            node.attribute("UnificationType").unwrap_or("SAME_PLAYER"),
            &["SAME_PLAYER", "ALLIED_PLAYERS"],
            path,
        )?,
    );
    let capture = Table::record(
        std::iter::once(behavior)
            .chain(fields(
                start,
                &[
                    ("GrantMoney", 0x7631_77a4, 8),
                    ("GrantMoneyOneTime", 0x77e3_f4f7, 0x10),
                    ("SlavedCaptureObject", 0x148e_d75d, 0xc),
                ],
            ))
            .collect(),
    );
    let unified = Table::record(vec![
        base(0xf8c4_aa6b, capture),
        Entry::named("UnificationType", 0x19b6_94b8, Target::Data(start + 0x14)),
    ]);
    let upgrades: Vec<_> = significant_children(node).collect();
    if upgrades.is_empty() {
        return Err(invalid(
            path,
            "CaptureAndGiveUpgradeBehavior requires UpgradeMap",
        ));
    }
    let list = start + 0x18;
    let first = data.list(list, upgrades.len(), 8)?;
    let mut items = Vec::with_capacity(upgrades.len());
    for (index, upgrade) in upgrades.into_iter().enumerate() {
        if !upgrade.has_tag_name((SAGE_NS, "UpgradeMap")) || has_content(upgrade) {
            return Err(invalid(
                path,
                "invalid CaptureAndGiveUpgradeBehavior UpgradeMap",
            ));
        }
        validate_attributes(upgrade, &["OwnershipAmount", "ProvidedUpgrade"], path)?;
        let offset = first + 8 * index;
        data.word(
            offset,
            sage_int(
                upgrade.attribute("OwnershipAmount").unwrap_or("0"),
                "OwnershipAmount",
                path,
            )?,
        );
        data.word(
            offset + 4,
            reference(
                upgrade.attribute("ProvidedUpgrade").unwrap_or(""),
                "UpgradeTemplate",
                path,
                references,
                reference_types,
            )?,
        );
        items.push((
            0,
            Target::Table(Table::record(
                fields(
                    offset,
                    &[
                        ("OwnershipAmount", 0x23e6_50b8, 0),
                        ("ProvidedUpgrade", 0x4aba_1f29, 4),
                    ],
                )
                .collect(),
            )),
        ));
    }
    Ok((
        KIND,
        Table::record(vec![
            base(0xbd7d_7cde, unified),
            Entry::named(
                "UpgradeMap",
                0x8493_e45a,
                Target::Table(Table::array(list, items)),
            ),
        ]),
    ))
}
