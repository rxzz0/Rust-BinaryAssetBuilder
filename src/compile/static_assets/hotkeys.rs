use super::{
    Marshaled, Node, Path, SAGE_NS, asset_reference, encode_words, fast_hash, has_content, invalid,
    io, production_queue_type, required, required_element, significant_children, text_value,
    validate_attributes,
};

const SLOT: &str = "HotKeySlot";
const KEY: &str = "MappableKey";

pub(super) fn mappable_key(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "KeyDef"], path)?;
    let key = required(node, "KeyDef", path)?;
    string_asset(node, path, &[0, key.len() as u32, 12], key, 8)
}

pub(super) fn slot(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const GROUPS: &[&str] = &[
        "Camera",
        "SideBar",
        "UnitCommands",
        "Bookmarks",
        "PlayerPowersBar",
        "Misc",
        "NonConfigurable",
        "Cheat",
    ];
    validate_attributes(
        node,
        &["id", "Group", "Index", "Name", "VersionAdded"],
        path,
    )?;
    let group = enum_value(node.attribute("Group").unwrap_or("Misc"), GROUPS, path)?;
    let index = int_attribute(node, "Index", "-1", path)?;
    let name = required(node, "Name", path)?;
    let version = int_attribute(node, "VersionAdded", "1", path)?;
    string_asset(
        node,
        path,
        &[0, group, index, name.len() as u32, 24, version],
        name,
        16,
    )
}

pub(super) fn tactical(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "SelectionRefinementPageSlot"], path)?;
    let mut data = Marshaled::default();
    let mut words = vec![0; 6];
    if let Some(value) = node.attribute("SelectionRefinementPageSlot") {
        words[1] = import(&mut data, value, SLOT, 4, path)?;
        data.reference_types.push(fast_hash(SLOT.as_bytes()));
    }
    let mut children = significant_children(node).peekable();
    while let Some(page) = children.next_if(|child| child.has_tag_name((SAGE_NS, "BuildQueuePage")))
    {
        validate_attributes(page, &["QueueType", "Slot"], path)?;
        if has_content(page) {
            return Err(invalid(path, "unexpected content in BuildQueuePage"));
        }
        let queue = production_queue_type(required(page, "QueueType", path)?, "QueueType", path)?;
        words.push(queue);
        let slot = match page.attribute("Slot") {
            Some(value) => {
                data.reference_types.push(fast_hash(SLOT.as_bytes()));
                import(&mut data, value, SLOT, words.len() as u32 * 4, path)?
            }
            None => 0,
        };
        words.push(slot);
        words[2] += 1;
    }
    if words[2] != 0 {
        words[3] = 24;
        data.relocations.push(12);
    }
    let buttons_start = words.len() as u32 * 4;
    for button in children {
        if !button.has_tag_name((SAGE_NS, "ButtonSlot")) {
            return Err(invalid(path, "expected ButtonSlot element"));
        }
        let value = text_value(button, path)?;
        data.reference_types.push(fast_hash(SLOT.as_bytes()));
        words.push(import(
            &mut data,
            value,
            SLOT,
            words.len() as u32 * 4,
            path,
        )?);
        words[4] += 1;
    }
    if words[4] != 0 {
        words[5] = buttons_start;
        data.relocations.push(20);
    }
    Ok(finish(data, words))
}

pub(super) fn defaults(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let map = required_element(&mut children, "Map", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in DefaultHotKeys"));
    }
    validate_attributes(map, &[], path)?;
    let mut data = Marshaled::default();
    let mut words = vec![0; 3];
    for hotkey in significant_children(map) {
        if !hotkey.has_tag_name((SAGE_NS, "HotKey")) || has_content(hotkey) {
            return Err(invalid(path, "expected an empty HotKey element"));
        }
        validate_attributes(hotkey, &["Slot", "Key", "Modifiers"], path)?;
        for attribute in hotkey.attributes() {
            match attribute.name() {
                "Slot" => data.reference_types.push(fast_hash(SLOT.as_bytes())),
                "Key" => data.reference_types.push(fast_hash(KEY.as_bytes())),
                _ => {}
            }
        }
        for (field, target) in [("Slot", SLOT), ("Key", KEY)] {
            let value = required(hotkey, field, path)?;
            words.push(import(
                &mut data,
                value,
                target,
                words.len() as u32 * 4,
                path,
            )?);
        }
        let modifiers = hotkey
            .attribute("Modifiers")
            .unwrap_or("")
            .split_ascii_whitespace();
        let mut flags = 0;
        for modifier in modifiers {
            flags |= 1 << enum_value(modifier, &["CTRL", "ALT", "SHIFT"], path)?;
        }
        words.push(flags);
        words[1] += 1;
    }
    if words[1] != 0 {
        words[2] = 12;
        data.relocations.push(8);
    }
    Ok(finish(data, words))
}

fn import(
    data: &mut Marshaled,
    value: &str,
    target: &str,
    offset: u32,
    path: &Path,
) -> io::Result<u32> {
    data.references
        .push(asset_reference(value, target, path)?.into());
    data.imports.push(offset);
    Ok(data.references.len() as u32)
}

fn finish(mut data: Marshaled, words: Vec<u32>) -> Marshaled {
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    if !data.imports.is_empty() {
        data.imports.push(u32::MAX);
    }
    data.payload = encode_words(words);
    data
}

fn string_asset(
    node: Node<'_, '_>,
    path: &Path,
    header: &[u32],
    text: &str,
    relocation: u32,
) -> io::Result<Marshaled> {
    if has_content(node) {
        return Err(invalid(path, "unexpected content in hotkey asset"));
    }
    let mut payload = encode_words(header.iter().copied());
    payload.extend_from_slice(text.as_bytes());
    payload.resize((payload.len() + 1).next_multiple_of(4), 0);
    Ok(Marshaled {
        payload,
        relocations: vec![relocation, u32::MAX],
        ..Marshaled::default()
    })
}

fn int_attribute(node: Node<'_, '_>, name: &str, default: &str, path: &Path) -> io::Result<u32> {
    node.attribute(name)
        .unwrap_or(default)
        .trim()
        .parse::<i32>()
        .map(|value| value as u32)
        .map_err(|_| invalid(path, format!("invalid {name}")))
}

fn enum_value(value: &str, values: &[&str], path: &Path) -> io::Result<u32> {
    values
        .iter()
        .position(|item| *item == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("invalid hotkey value {value:?}")))
}
