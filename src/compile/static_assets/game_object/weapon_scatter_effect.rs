use std::io;

use roxmltree::Node;

use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, required_element, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::boolean;
use super::parking::encode_inline_filter;
use super::passive_area::reference;
use crate::compile::{has_content, invalid};

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x3eb8_b5d6;
    let path = data.path;
    validate_attributes(node, &["id", "WeaponTemplate", "RandomScatter3D"], path)?;
    let mut children = significant_children(node);
    let settings = required_element(&mut children, "ScatterSettings", path)?;
    let filter = required_element(&mut children, "Filter", path)?;
    validate_attributes(settings, &["ScatterAmount", "ScatterDelay"], path)?;
    if children.next().is_some() || has_content(settings) {
        return Err(invalid(
            path,
            "unexpected WeaponScatterEffectUpdate content",
        ));
    }

    let start = data.allocate(pointer, 0x88)?;
    let base = module_base(node, KIND, start, data, true)?;
    let weapon = if let Some(value) = node.attribute("WeaponTemplate") {
        let id = reference(value, "WeaponTemplate", path, references, reference_types)?;
        let offset = data.allocate(start + 8, 4)?;
        data.word(offset, id);
        Target::Data(offset)
    } else {
        Target::Null
    };

    let amount = settings
        .attribute("ScatterAmount")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid ScatterAmount"))?;
    data.word(start + 0xc, amount as u32);
    let delay = settings.attribute("ScatterDelay").unwrap_or("0s");
    data.word(
        start + 0x10,
        sage_time(delay, "ScatterDelay", path)?.to_bits(),
    );
    data.payload[start + 0x84] = boolean(node, "RandomScatter3D", false, path)?;

    let settings = Table::record(vec![
        Entry::named("ScatterAmount", 0x23e6_50b8, Target::Data(start + 0xc)),
        Entry::named("ScatterDelay", 0x9986_69e8, Target::Data(start + 0x10)),
    ]);
    let filter = encode_inline_filter(filter, start + 0x14, data, reference_types)?;
    Ok((
        KIND,
        Table::record(vec![
            base,
            Entry::named("WeaponTemplate", 0xd046_dd22, weapon),
            Entry::named("RandomScatter3D", 0x77e3_f4f7, Target::Data(start + 0x84)),
            Entry::named("ScatterSettings", 0x9e23_34fd, Target::Table(settings)),
            Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)),
        ]),
    ))
}
