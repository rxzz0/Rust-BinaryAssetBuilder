use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::global_ui::enum_value;
use super::{
    Marshaled, sage_int, significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn cursor(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const DIRECTIONS: &str = "NumDirections";
    validate_leaf(node, &["id", "File", DIRECTIONS], path)?;
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    if let Some(file) = node.attribute("File") {
        data.string(4, file)?;
    }
    let directions = sage_int(node.attribute(DIRECTIONS).unwrap_or("1"), DIRECTIONS, path)?;
    data.word(12, directions);
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn basic_cursors(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    cursor_references(node, &["Normal", "Arrow", "Invalid", "EnterText"], path)
}

pub(super) fn tactical_cursor_layout(type_name: &str) -> Option<(u32, &'static [&'static str])> {
    Some(match type_name.strip_prefix("UIMouseTacticalCommand")? {
        "Attack" => (
            0x02bd_5240,
            &[
                "AttackObjectCursor",
                "ForceAttackObjectCursor",
                "ForceAttackGroundCursor",
                "ExplosiveChargeCursor",
                "ClearGarrisonCursor",
                "DisguiseCursor",
                "CallAirstrikeCursor",
            ],
        ),
        "AttackMove" => (0xd265_7960, &["NormalCursor", "ForcedCursor"]),
        "Dock" => (0xacdd_2f29, &["Cursor"]),
        "ForceAttack" => (0xd046_99e4, &["ObjectCursor", "GroundCursor"]),
        "ForceMove" => (0x07bf_94d3, &["Cursor"]),
        "GetCrate" => (0x8e7a_0fa6, &["Cursor"]),
        "Harvest" => (0x600a_25d0, &["Cursor"]),
        "MineOre" => (0xb3dd_a9e2, &["Cursor"]),
        "Misc" => (
            0x05be_70d4,
            &[
                "CombineCursor",
                "ChargeDefensesCursor",
                "ConversionBeamCursor",
            ],
        ),
        "Move" => (0xc0f9_04a1, &["ForwardCursor", "ReverseCursor"]),
        "PlaceRallyPoint" => (0x417e_47cc, &["Cursor"]),
        "PlaceWall" => (0x5bf6_293c, &["ValidPlacementCursor"]),
        "ReverseMove" => (0xe7d4_dd88, &["Cursor"]),
        "SetRallyPoint" => (0xce71_e899, &["Cursor"]),
        "ToggleSelfRepair" => (0x295a_4872, &["ValidTargetCursor", "InvalidTargetCursor"]),
        _ => return None,
    })
}

pub(super) fn tactical_cursors(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let name = node.tag_name().name();
    let (_, fields) = tactical_cursor_layout(name)
        .ok_or_else(|| invalid(path, format!("unsupported tactical cursor command {name}")))?;
    cursor_references(node, fields, path)
}

fn cursor_references(node: Node<'_, '_>, fields: &[&str], path: &Path) -> io::Result<Marshaled> {
    let allowed: Vec<_> = std::iter::once("id")
        .chain(fields.iter().copied())
        .collect();
    validate_leaf(node, &allowed, path)?;
    let mut data = BinaryData {
        payload: vec![0; 4 + fields.len() * 4],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for attribute in node.attributes() {
        if let Some(index) = fields.iter().position(|&name| name == attribute.name()) {
            write_import(
                attribute.value(),
                "MouseCursor",
                4 + index * 4,
                &mut data,
                &mut result,
            )?;
        }
    }
    Ok(data.finish(result))
}

pub(super) fn image_sequence(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let frames: Vec<_> = significant_children(node).collect();
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let start = data.list(4, frames.len(), 4)?;
    for (index, frame) in frames.into_iter().enumerate() {
        if !frame.has_tag_name((SAGE_NS, "Image")) {
            return Err(invalid(path, "expected an ImageSequence Image element"));
        }
        write_import(
            text_value(frame, path)?,
            "PackedTextureImage",
            start + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    Ok(data.finish(result))
}

pub(super) fn bootup(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const LOADING_TEXTURE: &str = "LoadingTextTexture";
    validate_attributes(node, &["id", LOADING_TEXTURE], path)?;
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    write_import(
        required(node, LOADING_TEXTURE, path)?,
        "Texture",
        4,
        &mut data,
        &mut result,
    )?;
    let items: Vec<_> = significant_children(node).collect();
    let start = data.list(8, items.len(), 28)?;
    let items_end = data.payload.len();
    for (index, item) in items.into_iter().enumerate() {
        let offset = start + index * 28;
        write_bootup_item(item, offset, &mut data, &mut result)?;
    }
    // EA records pointers in the display items before the nested movie string pointers.
    data.relocations
        .sort_by_key(|&offset| offset as usize >= items_end);
    Ok(data.finish(result))
}

fn write_bootup_item(
    item: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    const TYPES: &[&str] = &["STATIC_SCREEN", "LOAD_SCREEN", "MOVIE"];
    const REBOOT: &str = "ShowOnReboot";
    let path = data.path;
    if !item.has_tag_name((SAGE_NS, "DisplayItem")) {
        return Err(invalid(
            path,
            "expected a BootupDisplaySequence DisplayItem",
        ));
    }
    validate_attributes(item, &["Type", "Duration", "Priority", REBOOT], path)?;
    let kind = enum_value(required(item, "Type", path)?, TYPES, path)?;
    let duration = sage_int(item.attribute("Duration").unwrap_or("0"), "Duration", path)?;
    let priority = sage_int(required(item, "Priority", path)?, "Priority", path)?;
    required(item, REBOOT, path)?;
    data.word(offset, kind);
    data.word(offset + 4, duration);
    data.word(offset + 8, priority);
    data.payload[offset + 24] = boolean(item, REBOOT, false, path)?;

    let mut children = significant_children(item).peekable();
    let mut textures = Vec::new();
    while children
        .peek()
        .is_some_and(|child| child.has_tag_name((SAGE_NS, "RandomTexture")))
    {
        let texture = children.next().expect("peeked RandomTexture");
        textures.push(text_value(texture, path)?);
    }
    let start = data.list(offset + 12, textures.len(), 4)?;
    for (index, texture) in textures.into_iter().enumerate() {
        write_import(texture, "Texture", start + index * 4, data, result)?;
    }
    if let Some(movie) = children.next() {
        if !movie.has_tag_name((SAGE_NS, "Movie"))
            || movie.children().any(|child| child.is_element())
            || children.next().is_some()
        {
            return Err(invalid(
                path,
                "expected a single optional DisplayItem Movie element",
            ));
        }
        validate_attributes(movie, &[], path)?;
        let text: String = movie
            .children()
            .filter(Node::is_text)
            .filter_map(|child| child.text())
            .collect();
        let string = data.allocate(offset + 20, 8)?;
        data.string(string, &text)?;
    }
    Ok(())
}
