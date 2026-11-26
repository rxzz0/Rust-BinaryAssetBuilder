use super::binary::BinaryData;
use super::{
    Marshaled, ai_bank_account, fx_list, required_element, sage_time, significant_children,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn radar_terrain_image(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "MapBorder", "Texture", "Rotated"], path)?;
    let border = match node.attribute("MapBorder").unwrap_or("Orange").trim() {
        "Invalid" => u32::MAX,
        "Orange" => 0,
        "Green" => 1,
        "Blue" => 2,
        "Cyan" => 3,
        "Magenta" => 4,
        "Yellow" => 5,
        "Purple" => 6,
        "Pink" => 7,
        _ => return Err(invalid(path, "invalid radar terrain image MapBorder")),
    };
    let mut data = BinaryData {
        payload: vec![0; 40],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.word(4, border);
    write_import(
        required(node, "Texture", path)?,
        "Texture",
        8,
        &mut data,
        &mut result,
    )?;
    let mut children = significant_children(node);
    for (index, name) in ["ImageSize", "ImagePosition", "TextureSize"]
        .into_iter()
        .enumerate()
    {
        let coordinates = required_element(&mut children, name, path)?;
        validate_attributes(coordinates, &["x", "y"], path)?;
        if has_content(coordinates) {
            return Err(invalid(path, format!("unexpected content in {name}")));
        }
        for (axis, field) in ["x", "y"].into_iter().enumerate() {
            let value = coordinates
                .attribute(field)
                .unwrap_or("0")
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name} {field} coordinate")))?;
            data.word(12 + index * 8 + axis * 4, value as u32);
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in RadarTerrainImage"));
    }
    data.payload[36] = fx_list::boolean(node, "Rotated", false, path)?;
    Ok(data.finish(result))
}

pub(super) fn skirmish_opening_move(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Side"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 20],
        relocations: Vec::new(),
        path,
    };
    data.string(4, required(node, "Side", path)?)?;
    let orders = significant_children(node).collect::<Vec<_>>();
    let start = data.list(12, orders.len(), 20)?;
    for (index, order) in orders.into_iter().enumerate() {
        if !order.has_tag_name((SAGE_NS, "Order")) || has_content(order) {
            return Err(invalid(path, "expected SkirmishOpeningMove/Order"));
        }
        validate_attributes(order, &["Time", "Build", "Account", "Deposit"], path)?;
        let time = sage_time(
            order.attribute("Time").unwrap_or("0s"),
            "opening move Time",
            path,
        )?;
        let build = order.attribute("Build").unwrap_or("");
        let account = ai_bank_account(order.attribute("Account").unwrap_or("SLUSH_FUND"), path)?;
        let deposit = order
            .attribute("Deposit")
            .unwrap_or("0")
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, "invalid opening move Deposit"))?;
        let offset = start + index * 20;
        data.word(offset, time.to_bits());
        data.string(offset + 4, build)?;
        data.word(offset + 12, account);
        data.word(offset + 16, deposit);
    }
    Ok(data.finish(Marshaled::default()))
}
