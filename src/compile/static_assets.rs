use super::{
    AssetReference, CompiledAsset, SAGE_NS, has_content, hash_xml, invalid, memory_stream_hash,
    required,
};
use crate::hash::fast_hash;
use crate::manifest::AssetId;
use roxmltree::Node;
use std::io;
use std::path::Path;

mod hotkeys;

type Marshal = fn(Node<'_, '_>, &Path) -> io::Result<Marshaled>;

fn encoder(type_name: &str) -> Option<(u32, u8, Marshal)> {
    let encoder: (u32, u8, Marshal) = match type_name {
        "AIBudgetStateDefinition" => (0x99ff_1736, 0, ai_budget),
        "PlayerTechRank" => (0x1227_1bce, 1, player_tech_rank),
        "Video" => (0x8433_efa6, 2, video),
        "UnitIntro" => (0x5708_06d2, 3, unit_intro),
        "VideoEventList" => (0x96da_1c03, 4, video_events),
        "CampaignEnumerator" => (0x0714_c282, 5, campaign_enumerator),
        "LogicCommandSet" => (0x8ae8_b68c, 6, logic_commands),
        "W3DCollisionBox" => (0x6e26_f699, 7, w3d_collision_box),
        "W3DHierarchy" => (0xed94_de61, 8, w3d_hierarchy),
        "MultiplayerColor" => (0x131c_20a0, 9, multiplayer_color),
        "HotKeySlot" => (0x93e2_3672, 10, hotkeys::slot),
        "MappableKey" => (0x84b6_aec1, 11, hotkeys::mappable_key),
        "UIMouseTacticalMainCommandBarHotKeys" => (0x73e3_11a9, 12, hotkeys::tactical),
        "DefaultHotKeys" => (0x4949_5abd, 13, hotkeys::defaults),
        "MpGameRules" => (0x34d9_1648, 14, mp_game_rules),
        _ => return None,
    };
    Some(encoder)
}

pub(super) fn compile_asset(
    node: Node<'_, '_>,
    path: &Path,
    source: &str,
) -> io::Result<CompiledAsset> {
    let type_name = node.tag_name().name();
    let (id, asset_id) = declaration_id(node, path)?;
    if let Some(base) = node.attribute("inheritFrom") {
        return Err(invalid(
            path,
            format!("unsupported inheritance {type_name}:{id} from {base}"),
        ));
    }
    let (type_hash, sort_order, marshal) = encoder(type_name)
        .ok_or_else(|| invalid(path, format!("unsupported static asset type {type_name}")))?;
    let data = marshal(node, path)?;
    let mut instance_hash = hash_xml(node, type_hash ^ 4, path)?;
    if !data.reference_types.is_empty() {
        instance_hash ^= memory_stream_hash(encode_words(data.reference_types));
    }
    Ok(CompiledAsset {
        id: asset_id,
        type_hash,
        instance_hash,
        name: format!("{type_name}:{id}"),
        source: source.to_owned(),
        payload: data.payload,
        relocations: encode_words(data.relocations),
        imports: encode_words(data.imports),
        references: data.references,
        sort_order,
    })
}

pub(super) fn declaration_id<'a>(
    node: Node<'a, '_>,
    path: &Path,
) -> io::Result<(&'a str, AssetId)> {
    let type_name = node.tag_name().name();
    if node.tag_name().namespace() != Some(SAGE_NS) {
        return Err(invalid(
            path,
            format!("unsupported static asset type {type_name}"),
        ));
    }
    let id = required(node, "id", path)?;
    if id.contains(':') {
        return Err(invalid(path, "invalid asset ID"));
    }
    Ok((
        id,
        AssetId {
            type_id: fast_hash(type_name.as_bytes()),
            instance_id: hash_name(id),
        },
    ))
}

#[derive(Default)]
struct Marshaled {
    payload: Vec<u8>,
    relocations: Vec<u32>,
    imports: Vec<u32>,
    references: Vec<AssetReference>,
    reference_types: Vec<u32>,
}

fn validate_attributes(node: Node<'_, '_>, allowed: &[&str], path: &Path) -> io::Result<()> {
    for attribute in node.attributes() {
        if attribute.namespace().is_some() || !allowed.contains(&attribute.name()) {
            return Err(invalid(
                path,
                format!(
                    "unsupported {} attribute {}",
                    node.tag_name().name(),
                    attribute.name()
                ),
            ));
        }
    }
    Ok(())
}

fn video(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &["id", "Filename", "Comment", "Volume", "SoundOverride"],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in Video"));
    }
    let filename = required(node, "Filename", path)?;
    let comment = node.attribute("Comment").unwrap_or("No Comment");
    if !comment
        .bytes()
        .all(|byte| byte == b' ' || byte.is_ascii_graphic())
    {
        return Err(invalid(path, "Video comment must be ASCII"));
    }
    let volume = percentage(
        node.attribute("Volume").unwrap_or("80"),
        "Video volume",
        path,
    )?;
    let sound = match node.attribute("SoundOverride").unwrap_or("DUCK") {
        "DUCK" => 0,
        "NO_DUCK" => 1,
        _ => return Err(invalid(path, "invalid Video sound override")),
    };
    let comment_offset = 28 + (filename.len() + 1).next_multiple_of(4) as u32;
    let mut payload = encode_words([
        0,
        filename.len() as u32,
        28,
        comment.len() as u32,
        comment_offset,
        volume.to_bits(),
        sound,
    ]);
    for text in [filename, comment] {
        payload.extend_from_slice(text.as_bytes());
        payload.resize((payload.len() + 1).next_multiple_of(4), 0);
    }
    Ok(Marshaled {
        payload,
        relocations: vec![8, 16, u32::MAX],
        ..Marshaled::default()
    })
}

fn ai_budget(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = node.children().filter(Node::is_element);
    let heuristic = children
        .next()
        .filter(|node| node.has_tag_name((SAGE_NS, "Heuristic")))
        .ok_or_else(|| invalid(path, "expected AIBudgetStateDefinition/Heuristic"))?;
    let budget = children
        .next()
        .filter(|node| node.has_tag_name((SAGE_NS, "Budget")))
        .ok_or_else(|| invalid(path, "expected AIBudgetStateDefinition/Budget"))?;
    if children.next().is_some()
        || heuristic.attributes().len() != 0
        || budget.attributes().len() != 0
    {
        return Err(invalid(path, "invalid AIBudgetStateDefinition"));
    }

    let mut shares = Vec::new();
    for share in budget.children().filter(Node::is_element) {
        if !share.has_tag_name((SAGE_NS, "AccountShare")) || has_content(share) {
            return Err(invalid(path, "expected Budget/AccountShare"));
        }
        validate_attributes(share, &["Account", "Percent"], path)?;
        let account = match share.attribute("Account").unwrap_or("INVESTMENT") {
            "INVESTMENT" => 0,
            "SPECIAL_OPERATIONS" => 1,
            "PRODUCTION" => 2,
            "DEFENSE" => 3,
            "TECHNOLOGY" => 4,
            "SLUSH_FUND" => 5,
            _ => return Err(invalid(path, "invalid AI bank account")),
        };
        let percent = percentage(
            share.attribute("Percent").unwrap_or("0"),
            "AI bank account share",
            path,
        )?;
        shares.extend([account, percent.to_bits()]);
    }

    let mut weights = Vec::new();
    for choice in heuristic.children().filter(Node::is_element) {
        if !choice.has_tag_name((SAGE_NS, "ConstantHeuristic")) || has_content(choice) {
            return Err(invalid(
                path,
                "only ConstantHeuristic is supported for AI budgets",
            ));
        }
        validate_attributes(choice, &["Weight"], path)?;
        let weight = choice.attribute("Weight").unwrap_or("1.0");
        weights.push(finite_number(weight, "AI heuristic weight", path)?.to_bits());
    }
    if weights.is_empty() {
        return Err(invalid(path, "expected at least one AI budget heuristic"));
    }
    let count = weights.len() as u32;
    let heuristic_offset = 20 + 4 * shares.len() as u32;
    let mut words = vec![
        0,
        count,
        heuristic_offset,
        (shares.len() / 2) as u32,
        if shares.is_empty() { 0 } else { 20 },
    ];
    let mut relocations = Vec::new();
    if !shares.is_empty() {
        relocations.push(16);
    }
    words.extend(shares);
    relocations.push(8);
    let values_offset = heuristic_offset + 4 * count;
    for index in 0..count {
        words.push(values_offset + 8 * index);
        relocations.push(heuristic_offset + 4 * index);
    }
    let constant_type = fast_hash(b"AIStateConstantHeuristic");
    for weight in weights {
        words.extend([constant_type, weight]);
    }
    relocations.push(u32::MAX);
    Ok(Marshaled {
        payload: encode_words(words),
        relocations,
        ..Marshaled::default()
    })
}

fn percentage(value: &str, field: &str, path: &Path) -> io::Result<f32> {
    let value = value.trim();
    Ok(finite_number(value.strip_suffix('%').unwrap_or(value), field, path)? * 0.01)
}

fn finite_number(value: &str, field: &str, path: &Path) -> io::Result<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or_else(|| invalid(path, format!("invalid {field}")))
}

fn mp_game_rules(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const TRANSFER: &str = "NumMinutesBeforePlayersCanTransferMoney";
    const CASH_FIELDS: [(&str, &str); 4] = [
        ("LoCash", "0"),
        ("HiCash", "0"),
        ("ChoiceStepAmount", "0"),
        ("DefaultChoiceIndex", "1"),
    ];
    validate_attributes(node, &["id", TRANSFER], path)?;
    let mut children = significant_children(node);
    let cash = required_element(&mut children, "SkirmishStartCash", path)?;
    validate_attributes(cash, &CASH_FIELDS.map(|(field, _)| field), path)?;
    if has_content(cash) || children.next().is_some() {
        return Err(invalid(path, "unexpected content in MpGameRules"));
    }
    let read = |node: Node<'_, '_>, field, default| {
        node.attribute(field)
            .unwrap_or(default)
            .trim()
            .parse::<i32>()
            .map(|value| value as u32)
            .map_err(|_| invalid(path, format!("invalid MpGameRules {field}")))
    };
    let mut words = [0; 6];
    words[1] = read(node, TRANSFER, "5")?;
    for (word, (field, default)) in words[2..].iter_mut().zip(CASH_FIELDS) {
        *word = read(cash, field, default)?;
    }
    Ok(Marshaled {
        payload: encode_words(words),
        ..Marshaled::default()
    })
}

fn w3d_collision_box(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "JoypadPickingOnly"], path)?;
    let mut children = significant_children(node);
    let mut words = [0; 8];
    for (name, offset) in [("Center", 1), ("Extent", 4)] {
        let vector = required_element(&mut children, name, path)?;
        validate_attributes(vector, &["X", "Y", "Z"], path)?;
        if has_content(vector) {
            return Err(invalid(path, format!("unexpected content in {name}")));
        }
        for (component, axis) in words[offset..offset + 3].iter_mut().zip(["X", "Y", "Z"]) {
            *component = finite_number(required(vector, axis, path)?, axis, path)?.to_bits();
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in W3DCollisionBox"));
    }
    words[7] = match node
        .attribute("JoypadPickingOnly")
        .unwrap_or("false")
        .trim()
    {
        "false" | "0" => 0,
        "true" | "1" => 1,
        _ => return Err(invalid(path, "invalid W3DCollisionBox JoypadPickingOnly")),
    };
    Ok(Marshaled {
        payload: encode_words(words),
        ..Marshaled::default()
    })
}

fn significant_children<'a, 'input>(
    node: Node<'a, 'input>,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(|child| {
        child.is_element()
            || child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
    })
}

fn required_element<'a, 'input>(
    children: &mut impl Iterator<Item = Node<'a, 'input>>,
    name: &str,
    path: &Path,
) -> io::Result<Node<'a, 'input>> {
    children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, name)))
        .ok_or_else(|| invalid(path, format!("expected {name} element")))
}

fn w3d_hierarchy(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const MATRIX: &[&str] = &[
        "M00", "M10", "M20", "M30", "M01", "M11", "M21", "M31", "M02", "M12", "M22", "M32", "M03",
        "M13", "M23", "M33",
    ];
    validate_attributes(node, &["id"], path)?;
    let count = u32::try_from(significant_children(node).count())
        .map_err(|_| invalid(path, "too many W3DHierarchy pivots"))?;
    if count == 0 {
        return Err(invalid(path, "W3DHierarchy requires a Pivot"));
    }
    let mut words = vec![0, count, 12];
    for pivot in significant_children(node) {
        if !pivot.has_tag_name((SAGE_NS, "Pivot")) {
            return Err(invalid(path, "unexpected element in W3DHierarchy"));
        }
        validate_attributes(pivot, &["Name", "Parent"], path)?;
        let name = required(pivot, "Name", path)?;
        let parent = required(pivot, "Parent", path)?
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, "invalid W3DHierarchy Pivot Parent"))?;
        words.extend([hash_name(name), parent as u32]);
        let mut fields = significant_children(pivot);
        for (tag, attributes) in [
            ("Translation", &["X", "Y", "Z"][..]),
            ("Rotation", &["X", "Y", "Z", "W"][..]),
            ("FixupMatrix", MATRIX),
        ] {
            let field = required_element(&mut fields, tag, path)?;
            validate_attributes(field, attributes, path)?;
            if has_content(field) {
                return Err(invalid(path, format!("unexpected content in {tag}")));
            }
            for &attribute in attributes {
                let value = if tag == "FixupMatrix" {
                    field.attribute(attribute).unwrap_or("0")
                } else {
                    required(field, attribute, path)?
                };
                words.push(finite_number(value, attribute, path)?.to_bits());
            }
        }
        if fields.next().is_some() {
            return Err(invalid(path, "unexpected element in W3DHierarchy Pivot"));
        }
    }
    Ok(Marshaled {
        payload: encode_words(words),
        relocations: vec![8, u32::MAX],
        ..Marshaled::default()
    })
}

fn multiplayer_color(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node).peekable();
    let rgb = color4(required_element(&mut children, "RGBColor", path)?, path)?;
    let tint = children
        .next_if(|child| child.has_tag_name((SAGE_NS, "RGBColorT")))
        .map(|child| color4(child, path))
        .transpose()?
        .unwrap_or(0);
    let night = color4(
        required_element(&mut children, "RGBNightColor", path)?,
        path,
    )?;
    let tooltip = required_element(&mut children, "TooltipName", path)?;
    let available = required_element(&mut children, "AvailableInMetaGame", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected content in MultiplayerColor"));
    }
    let tooltip = text_value(tooltip, path)?;
    let length = u32::try_from(tooltip.len())
        .map_err(|_| invalid(path, "MultiplayerColor TooltipName is too long"))?;
    let available = match text_value(available, path)?.trim() {
        "false" | "0" => 0,
        "true" | "1" => 1,
        _ => {
            return Err(invalid(
                path,
                "invalid MultiplayerColor AvailableInMetaGame",
            ));
        }
    };
    let mut payload = encode_words([0, rgb, tint, night, length, 28, available]);
    payload.extend_from_slice(tooltip.as_bytes());
    payload.resize((payload.len() + 1).next_multiple_of(4), 0);
    Ok(Marshaled {
        payload,
        relocations: vec![20, u32::MAX],
        ..Marshaled::default()
    })
}

fn color4(node: Node<'_, '_>, path: &Path) -> io::Result<u32> {
    let channels = ["B", "G", "R", "A"];
    validate_attributes(node, &channels, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in color"));
    }
    let mut bytes = [0; 4];
    for (byte, channel) in bytes.iter_mut().zip(channels) {
        *byte = node
            .attribute(channel)
            .unwrap_or("0")
            .trim()
            .parse()
            .map_err(|_| invalid(path, format!("invalid color channel {channel}")))?;
    }
    Ok(u32::from_le_bytes(bytes))
}

fn text_value<'a>(node: Node<'a, '_>, path: &Path) -> io::Result<&'a str> {
    validate_attributes(node, &[], path)?;
    let mut children = significant_children(node);
    let value = children
        .next()
        .filter(Node::is_text)
        .and_then(|text| text.text())
        .filter(|value| value.is_ascii());
    match value {
        Some(value) if children.next().is_none() => Ok(value),
        _ => Err(invalid(
            path,
            format!("expected ASCII text in {}", node.tag_name().name()),
        )),
    }
}

fn unit_intro(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Video", "Audio"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in UnitIntro"));
    }
    let video = asset_reference(required(node, "Video", path)?, "VideoEventList", path)?;
    let audio = required(node, "Audio", path)?;
    let audio_id = asset_reference(audio, "AudioEvent", path)?;
    let base_audio_type = fast_hash(b"BaseAudioEventInfo");
    let reference_types = node
        .attributes()
        .filter_map(|attribute| match attribute.name() {
            "Video" => Some(video.type_id),
            "Audio" => Some(base_audio_type),
            _ => None,
        })
        .collect();
    Ok(Marshaled {
        payload: encode_words([0, video.instance_id, 1]),
        imports: vec![8, u32::MAX],
        references: vec![AssetReference {
            id: audio_id,
            external_type: (!audio.contains(':')).then_some(base_audio_type),
        }],
        reference_types,
        ..Marshaled::default()
    })
}

fn video_events(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = node.children().filter(Node::is_element);
    let events = children
        .next()
        .filter(|node| node.has_tag_name((SAGE_NS, "EventList")))
        .ok_or_else(|| invalid(path, "expected VideoEventList/EventList"))?;
    if children.next().is_some() || events.attributes().len() != 0 {
        return Err(invalid(path, "invalid VideoEventList"));
    }
    let mut references = Vec::new();
    for movie in events.children().filter(Node::is_element) {
        if !movie.has_tag_name((SAGE_NS, "Movie"))
            || movie.attributes().len() != 1
            || has_content(movie)
        {
            return Err(invalid(
                path,
                "only Movie events without fades are supported",
            ));
        }
        references
            .push(asset_reference(required(movie, "MovieAsset", path)?, "Video", path)?.into());
    }
    let count = references.len() as u32;
    if count == 0 {
        return Ok(Marshaled {
            payload: vec![0; 12],
            ..Marshaled::default()
        });
    }
    let start = 12 + 4 * count;
    let mut words = vec![0, count, 12];
    let mut relocations = vec![8];
    let mut imports = Vec::new();
    for index in 0..count {
        words.push(start + 16 * index);
        relocations.push(12 + 4 * index);
    }
    let movie_type = fast_hash(b"MovieEvent");
    for index in 0..count {
        words.extend([movie_type, 0, 0, index + 1]);
        imports.push(start + 16 * index + 12);
    }
    relocations.push(u32::MAX);
    imports.push(u32::MAX);
    Ok(Marshaled {
        payload: encode_words(words),
        relocations,
        imports,
        reference_types: vec![fast_hash(b"Video"); count as usize],
        references,
    })
}

fn player_tech_rank(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &["id", "ExperiencePointsToNextRank", "TechPointsGranted"],
        path,
    )?;
    let value = |name| {
        node.attribute(name)
            .unwrap_or("0")
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, format!("invalid {name}")))
    };
    reference_list(
        node,
        path,
        "TechGranted",
        "PlayerTech",
        vec![
            0,
            value("ExperiencePointsToNextRank")?,
            value("TechPointsGranted")?,
        ],
    )
}

fn campaign_enumerator(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let result = reference_list(node, path, "Campaign", "CampaignTemplate", vec![0])?;
    if result.references.is_empty() {
        return Err(invalid(
            path,
            "CampaignEnumerator requires at least one Campaign",
        ));
    }
    Ok(result)
}

fn logic_commands(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    reference_list(node, path, "Cmd", "LogicCommand", vec![0])
}

fn reference_list(
    node: Node<'_, '_>,
    path: &Path,
    element: &str,
    target_type: &str,
    mut words: Vec<u32>,
) -> io::Result<Marshaled> {
    let mut references = Vec::new();
    for child in node.children().filter(Node::is_element) {
        if !child.has_tag_name((SAGE_NS, element))
            || child.attributes().len() != 0
            || child.children().any(|node| node.is_element())
        {
            return Err(invalid(
                path,
                format!("expected a plain {element} reference"),
            ));
        }
        let value = child
            .text()
            .ok_or_else(|| invalid(path, format!("missing {element} value")))?;
        references.push(asset_reference(value, target_type, path)?.into());
    }
    let count = references.len() as u32;
    let pointer = (words.len() as u32 + 1) * 4;
    let list = pointer + 4;
    words.extend([count, if count == 0 { 0 } else { list }]);
    if count == 0 {
        return Ok(Marshaled {
            payload: encode_words(words),
            ..Marshaled::default()
        });
    }
    words.extend(1..=count);
    let mut imports: Vec<_> = (0..count).map(|index| list + 4 * index).collect();
    imports.push(u32::MAX);
    Ok(Marshaled {
        payload: encode_words(words),
        relocations: vec![pointer, u32::MAX],
        imports,
        references,
        reference_types: vec![fast_hash(target_type.as_bytes()); count as usize],
    })
}

fn asset_reference(value: &str, target_type: &str, path: &Path) -> io::Result<AssetId> {
    let value = value.trim();
    let (kind, name) = value.split_once(':').unwrap_or((target_type, value));
    if kind != target_type || name.is_empty() || name.contains(':') || !name.is_ascii() {
        return Err(invalid(
            path,
            format!("invalid {target_type} reference {value:?}"),
        ));
    }
    Ok(AssetId {
        type_id: fast_hash(kind.as_bytes()),
        instance_id: hash_name(name),
    })
}

fn hash_name(name: &str) -> u32 {
    fast_hash(name.to_ascii_lowercase().as_bytes())
}

fn encode_words(words: impl IntoIterator<Item = u32>) -> Vec<u8> {
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}
