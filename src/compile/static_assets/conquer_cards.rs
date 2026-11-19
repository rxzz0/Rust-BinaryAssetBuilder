use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::global_ui::enum_value;
use super::{
    Marshaled, required_element, sage_int, significant_children, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(in crate::compile) const BASE: &str = "ConquerCardDefinitionBase";
const UNIT: &str = "ConquerCardDefinitionUnit";
const STRUCTURE: &str = "ConquerCardDefinitionStructure";
const POWER: &str = "ConquerCardDefinitionPower";
pub(in crate::compile) const TYPES: &[&str] = &[UNIT, STRUCTURE, POWER];

pub(super) fn definition(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let (attribute, target, size) = match node.tag_name().name() {
        UNIT => ("Unit", "GameObject", 32),
        STRUCTURE => ("Structure", "GameObject", 16),
        _ => ("Power", "SpecialPowerTemplate", 24),
    };
    let mut fields = vec!["id", "Cost", "Maximum", attribute];
    if attribute == "Power" {
        fields.push("Side");
    }
    validate_attributes(node, &fields, path)?;
    let mut data = BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    data.word(4, sage_int(required(node, "Cost", path)?, "Cost", path)?);
    data.word(
        8,
        sage_int(node.attribute("Maximum").unwrap_or("0"), "Maximum", path)?,
    );
    write_import(
        required(node, attribute, path)?,
        target,
        12,
        &mut data,
        &mut out,
    )?;
    if attribute == "Unit" {
        write_unit_definition(node, &mut data, &mut out)?;
    } else {
        validate_leaf(node, &fields, path)?;
        if attribute == "Power" {
            data.string(16, required(node, "Side", path)?)?;
        }
    }
    Ok(data.finish(out))
}

fn write_unit_definition(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    let mut children = significant_children(node).peekable();
    let mut upgrades = Vec::new();
    while children
        .peek()
        .is_some_and(|child| child.has_tag_name((SAGE_NS, "Upgrade")))
    {
        upgrades.push(children.next().expect("peeked Upgrade"));
    }
    let start = data.list(16, upgrades.len(), 8)?;
    for (index, upgrade) in upgrades.into_iter().enumerate() {
        validate_leaf(upgrade, &["Upgrade", "Cost"], path)?;
        let offset = start + index * 8;
        write_import(
            required(upgrade, "Upgrade", path)?,
            "UpgradeTemplate",
            offset,
            data,
            out,
        )?;
        data.word(
            offset + 4,
            sage_int(required(upgrade, "Cost", path)?, "Cost", path)?,
        );
    }
    let ranks: Vec<_> = children.collect();
    let start = data.list(24, ranks.len(), 8)?;
    for (index, rank) in ranks.into_iter().enumerate() {
        if !rank.has_tag_name((SAGE_NS, "Veterancy")) {
            return Err(invalid(path, "expected a Conquer card Veterancy element"));
        }
        validate_leaf(rank, &["Veterancy", "Cost"], path)?;
        let offset = start + index * 8;
        data.word(offset, veterancy(required(rank, "Veterancy", path)?, path)?);
        data.word(
            offset + 4,
            sage_int(required(rank, "Cost", path)?, "Cost", path)?,
        );
    }
    Ok(())
}

pub(super) fn deck(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "DisplayName", "Description", "Side"], path)?;
    let mut children = significant_children(node);
    let cards = required_element(&mut children, "Cards", path)?;
    if children.next().is_some() {
        return Err(invalid(
            path,
            "expected a single Conquer deck Cards element",
        ));
    }
    validate_attributes(cards, &[], path)?;
    let cards: Vec<_> = significant_children(cards).collect();
    if cards.is_empty() {
        return Err(invalid(
            path,
            "Conquer deck Cards must contain at least one card",
        ));
    }
    let mut data = BinaryData {
        payload: vec![0; 36],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    data.string(4, required(node, "DisplayName", path)?)?;
    if let Some(description) = node.attribute("Description") {
        data.string(12, description)?;
    }
    data.string(20, required(node, "Side", path)?)?;
    let start = data.list(28, cards.len(), 4)?;
    for (index, card) in cards.into_iter().enumerate() {
        write_deck_card(card, start + index * 4, &mut data, &mut out)?;
    }
    data.relocations.sort_unstable();
    Ok(data.finish(out))
}

fn write_deck_card(
    card: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    if card.has_tag_name((SAGE_NS, "Structure")) || card.has_tag_name((SAGE_NS, "Power")) {
        validate_leaf(card, &["Card"], path)?;
        let start = data.allocate(pointer, 8)?;
        return write_import(required(card, "Card", path)?, BASE, start + 4, data, out);
    }
    if !card.has_tag_name((SAGE_NS, "Unit")) {
        return Err(invalid(
            path,
            "expected a Conquer deck Unit, Structure, or Power card",
        ));
    }
    validate_attributes(card, &["Card", "Count", "Veterancy"], path)?;
    let start = data.allocate(pointer, 24)?;
    data.word(start, 0x32a7_1db0);
    write_import(required(card, "Card", path)?, BASE, start + 4, data, out)?;
    data.word(
        start + 8,
        sage_int(card.attribute("Count").unwrap_or("1"), "Count", path)?,
    );
    data.word(
        start + 12,
        veterancy(card.attribute("Veterancy").unwrap_or("REGULAR"), path)?,
    );
    let upgrades: Vec<_> = significant_children(card).collect();
    let start = data.list(start + 16, upgrades.len(), 4)?;
    for (index, upgrade) in upgrades.into_iter().enumerate() {
        if !upgrade.has_tag_name((SAGE_NS, "Upgrade")) {
            return Err(invalid(
                path,
                "expected a Conquer deck Unit Upgrade element",
            ));
        }
        validate_leaf(upgrade, &["Upgrade"], path)?;
        write_import(
            required(upgrade, "Upgrade", path)?,
            "UpgradeTemplate",
            start + index * 4,
            data,
            out,
        )?;
    }
    Ok(())
}

fn veterancy(value: &str, path: &Path) -> io::Result<u32> {
    const RANKS: &[&str] = &["INVALID", "REGULAR", "VETERAN", "ELITE", "HEROIC"];
    Ok(enum_value(value, RANKS, path)?.wrapping_sub(1))
}
