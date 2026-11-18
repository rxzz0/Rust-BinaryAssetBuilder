use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::{
    Marshaled, finite_number, required_element, significant_children, text_value,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::{Attribute, Node};
use std::{io, path::Path};

pub(super) const STATE_KIND: &str = "ButtonSingleStateData";
pub(super) const TARGET_ATTRIBUTES: [&str; 5] = [
    "StateData",
    "ValidTargetCursor",
    "InvalidTargetCursor",
    "RadiusCursor",
    "ValidTargetOutline",
];
const TARGET_FIELDS: [(&str, &str, usize, Option<&str>); 4] = [
    ("StateData", STATE_KIND, 4, None),
    ("ValidTargetCursor", "MouseCursor", 8, Some("Arrow")),
    ("InvalidTargetCursor", "MouseCursor", 12, Some("Invalid")),
    ("RadiusCursor", "RadiusCursor", 16, None),
];
pub(super) const EXTRA_REFS: [(&str, &str, usize); 3] = [
    ("AdditionalSpecialPowerTemplate", "SpecialPowerTemplate", 28),
    ("AdditionalRadiusCursor", "RadiusCursor", 36),
    ("AdditionalValidTargetCursor", "MouseCursor", 44),
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "LogicCommand"], path)?;
    let variant = button_variant(node, path)?;
    let (kind, size) = match variant.tag_name().name() {
        "AssembleStructure" => (0x37be_b41d, 8),
        "BuildUnit" => (0x3ae9_97c4, 4),
        "BuildWall" => (0x3144_59f7, 8),
        "Evacuate" => (0x5347_6790, 8),
        "MultiplePowersTargetedSpecialPower" => (0xc678_a25e, 52),
        "ObjectUpgrade" => (0x2967_0e01, 8),
        "PlayerUpgrade" => (0x99e0_2045, 8),
        "SpecialPower" => (0x0db9_fd2e, 8),
        "TargetedSpecialPower" => (0x88ed_fb48, 28),
        "ToggleSpecialPower" => (0x5fc6_1e5c, 12),
        "Unpack" => (0xa16d_cfd5, 8),
        name => {
            return Err(invalid(
                path,
                format!("unsupported UnitAbilityButtonTemplate variant {name}"),
            ));
        }
    };

    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    write_import(
        required(node, "LogicCommand", path)?,
        "LogicCommand",
        4,
        &mut data,
        &mut out,
    )?;
    let start = data.allocate(8, size)?;
    data.word(start, kind);

    match variant.tag_name().name() {
        "BuildUnit" => validate_leaf(variant, &[], path)?,
        "ToggleSpecialPower" => toggle(variant, start, &mut data, &mut out)?,
        "TargetedSpecialPower" | "MultiplePowersTargetedSpecialPower" => {
            targeted(variant, start, size > 28, &mut data, &mut out)?;
        }
        _ => {
            validate_leaf(variant, &["StateData"], path)?;
            write_import(
                required(variant, "StateData", path)?,
                STATE_KIND,
                start + 4,
                &mut data,
                &mut out,
            )?;
        }
    }

    data.relocations.push(u32::MAX);
    out.imports.push(u32::MAX);
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}

pub(super) fn button_variant<'a, 'input>(
    node: Node<'a, 'input>,
    path: &Path,
) -> io::Result<Node<'a, 'input>> {
    let mut children = significant_children(node);
    let wrapper = required_element(&mut children, "Data", path)?;
    validate_attributes(wrapper, &[], path)?;
    let mut variants = significant_children(wrapper);
    variants
        .next()
        .filter(|child| child.tag_name().namespace() == Some(SAGE_NS))
        .filter(|_| children.next().is_none() && variants.next().is_none())
        .ok_or_else(|| {
            invalid(
                path,
                format!("{} requires one Data variant", node.tag_name().name()),
            )
        })
}

fn targeted(
    node: Node<'_, '_>,
    start: usize,
    multiple: bool,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    validate_attributes(node, &TARGET_ATTRIBUTES, data.path)?;
    required(node, "StateData", data.path)?;
    for attr in node.attributes() {
        write_target_reference(attr, start, data, out)?;
    }
    let refs = if multiple { &EXTRA_REFS[..] } else { &[] };
    finish_targeted(node, start, refs, data, out)
}

// EA assigns reference ordinals in XML attribute order, then adds defaults.
pub(super) fn write_target_reference(
    attr: Attribute<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    if let Some(&(_, kind, offset, _)) = TARGET_FIELDS.iter().find(|field| field.0 == attr.name()) {
        write_import(attr.value(), kind, start + offset, data, out)?;
    }
    Ok(())
}

pub(super) fn finish_targeted(
    node: Node<'_, '_>,
    start: usize,
    extra_refs: &[(&str, &str, usize)],
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    for (name, kind, offset, default) in TARGET_FIELDS {
        if let Some(value) = default.filter(|_| node.attribute(name).is_none()) {
            write_import(value, kind, start + offset, data, out)?;
        }
    }

    let outline = match node
        .attribute("ValidTargetOutline")
        .unwrap_or("HOVER_ATTACK")
        .trim()
    {
        "UNKNOWN" => u32::MAX,
        "NONE" => 0,
        "SELECTED" => 1,
        "HOVER_SELECT" => 2,
        "HOVER_ATTACK" => 3,
        value => {
            return Err(invalid(
                path,
                format!("invalid ValidTargetOutline {value:?}"),
            ));
        }
    };
    data.word(start + 20, outline);
    let mut children = significant_children(node).peekable();
    let tint = children.next_if(|child| child.has_tag_name((SAGE_NS, "ValidTargetTintColor")));
    for &(name, kind, offset) in extra_refs {
        let mut refs = Vec::new();
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            refs.push(text_value(child, path)?);
        }
        write_refs(&refs, kind, start + offset, data, out)?;
    }
    if children.next().is_some() {
        return Err(invalid(
            path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    if let Some(tint) = tint {
        let channels = ["R", "G", "B"];
        validate_leaf(tint, &channels, path)?;
        let color = data.allocate(start + 24, 12)?;
        for (index, name) in channels.into_iter().enumerate() {
            let value = finite_number(required(tint, name, path)?, name, path)?;
            data.word(color + index * 4, value.to_bits());
        }
    }
    Ok(())
}

fn toggle(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    validate_attributes(node, &[], data.path)?;
    let refs = significant_children(node)
        .map(|child| {
            if !child.has_tag_name((SAGE_NS, "StateData")) {
                return Err(invalid(data.path, "expected ToggleSpecialPower StateData"));
            }
            text_value(child, data.path)
        })
        .collect::<io::Result<Vec<_>>>()?;
    if refs.is_empty() {
        return Err(invalid(data.path, "ToggleSpecialPower requires StateData"));
    }
    write_refs(&refs, STATE_KIND, start + 4, data, out)
}

fn write_refs(
    refs: &[&str],
    kind: &str,
    count: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let first = data.list(count, refs.len(), 4)?;
    for (index, value) in refs.iter().enumerate() {
        write_import(value, kind, first + index * 4, data, out)?;
    }
    Ok(())
}
