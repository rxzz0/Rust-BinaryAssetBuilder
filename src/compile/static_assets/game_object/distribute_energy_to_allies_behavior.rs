use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{asset_reference, has_content, percentage, sage_uint, validate_attributes};
use super::behaviors_basic::{base, module_base};
use super::draws::{boolean, fields};
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x0ef8_19d2;
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "GrantMoney",
            "GrantMoneyOneTime",
            "SlavedCaptureObject",
            "ComradePercentage",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(
            path,
            "unexpected DistributeEnergyToAlliesBehavior content",
        ));
    }

    let start = data.allocate(pointer, 0x18)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    let money = sage_uint(
        node.attribute("GrantMoney").unwrap_or("0"),
        "GrantMoney",
        path,
    )?;
    data.word(start + 8, money);
    if let Some(value) = node
        .attribute("SlavedCaptureObject")
        .filter(|value| !value.trim().is_empty())
    {
        let id = asset_reference(value, "GameObject", path)?;
        data.word(start + 0xc, id.instance_id);
        reference_types.push(id.type_id);
    }
    data.payload[start + 0x10] = boolean(node, "GrantMoneyOneTime", false, path)?;
    let share = percentage(
        node.attribute("ComradePercentage").unwrap_or("100"),
        "ComradePercentage",
        path,
    )?;
    data.word(start + 0x14, share.to_bits());

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
    Ok((
        KIND,
        Table::record(vec![
            base(0xf8c4_aa6b, capture),
            Entry::named("ComradePercentage", 0x6458_64f3, Target::Data(start + 0x14)),
        ]),
    ))
}
