use super::{
    AssetReference, CompiledAsset, SAGE_NS, has_content, hash_xml, invalid, memory_stream_hash,
    required,
};
use crate::hash::fast_hash;
use crate::manifest::AssetId;
use roxmltree::Node;
use std::io;
use std::path::Path;

mod ability_buttons;
mod ai_build;
mod ai_command;
mod ai_heuristics;
mod ai_inheritance;
mod ai_personality;
mod ai_settings;
mod ai_strategic;
mod ai_targeting;
mod armor;
mod attribute_modifier;
mod audio;
mod audio_file;
mod audio_settings;
mod binary;
mod campaign;
mod credits;
mod damage_fx;
mod emotion;
mod eva;
mod experience_level;
mod fx_list;
mod game_lod;
mod game_map;
pub(super) mod game_object;
mod game_script_list;
mod global_ui;
mod hotkeys;
mod joypad_buttons;
mod locomotor;
pub(super) mod music;
pub(super) mod music_conditions;
mod object_creation;
mod particle_system;
mod player_power_button;
mod player_template;
mod road;
mod stance_template;
mod surface_mappings;
mod tokenized;
mod upgrade;
mod w3d_animation;
mod w3d_mesh;
mod weapon;

type Marshal = fn(Node<'_, '_>, &Path) -> io::Result<Marshaled>;

fn encoder(type_name: &str) -> Option<(u32, u8, Marshal)> {
    let encoder: (u32, u8, Marshal) = match type_name {
        "AIBudgetStateDefinition" => (0x99ff_1736, 0, ai_budget),
        "PlayerTechRank" => (0x1227_1bce, 1, player_tech_rank),
        "CameraSettings" => (0xfdcc_69de, 2, global_ui::camera),
        "Credits" => (0x10d6_57ee, 3, credits::credits),
        "HealthBarTemplate" => (0xaa4f_0d82, 4, global_ui::health_bar_template),
        "MpGameRules" => (0x34d9_1648, 5, mp_game_rules),
        "Video" => (0x8433_efa6, 6, video),
        "HealthBarSettings" => (0xf62c_9169, 7, global_ui::health_bar_settings),
        "UnitIntro" => (0x5708_06d2, 8, unit_intro),
        "UnitTypeIcon" => (0xf9e6_8b56, 52, global_ui::unit_type_icon),
        "ButtonSingleStateData" => (0xe15d_5b18, 62, global_ui::button_single_state),
        "UnitAbilityButtonTemplate" => (0xb087_4c15, 63, ability_buttons::compile),
        "ExperienceLevelTemplate" => (0x20d1_6b26, 64, experience_level::compile),
        "RadiusCursor" => (0xa0af_5530, 65, global_ui::radius_cursor),
        "PackedTextureImage" => (0xf8fc_ad30, 53, global_ui::packed_texture_image),
        "VideoEventList" => (0x96da_1c03, 9, video_events),
        "LogicCommandSet" => (0x8ae8_b68c, 11, logic_commands),
        "LogicCommand" => (0x5687_00a8, 49, logic_command),
        "W3DCollisionBox" => (0x6e26_f699, 12, w3d_collision_box),
        "W3DHierarchy" => (0xed94_de61, 13, w3d_hierarchy),
        "MultiplayerColor" => (0x131c_20a0, 14, multiplayer_color),
        "HotKeySlot" => (0x93e2_3672, 15, hotkeys::slot),
        "MappableKey" => (0x84b6_aec1, 16, hotkeys::mappable_key),
        "UIMouseTacticalMainCommandBarHotKeys" => (0x73e3_11a9, 17, hotkeys::tactical),
        "DefaultHotKeys" => (0x4949_5abd, 18, hotkeys::defaults),
        "AIPersonalityDefinition" => (0xcb3c_9d2b, 19, ai_personality::compile),
        "AIBuildStateDefinition" => (0xcaac_2e6e, 28, ai_build::compile),
        "AIStrategicStateDefinition" => (0xd63d_e874, 29, ai_strategic::compile),
        "AIData" => (0x4123_a1aa, 30, ai_settings::data),
        "AIMicroManagerData" => (0x52ff_0fd3, 31, ai_settings::micro),
        "AITargetingHeuristic" => (0x92a7_8f51, 32, ai_targeting::compile),
        "AttributeModifier" => (0xf901_fe9b, 51, attribute_modifier::compile),
        "ArmorTemplate" => (0x641a_d32e, 33, armor::compile),
        "UpgradeTemplate" => (0x4f8d_2969, 34, upgrade::compile),
        "EvaEvent" => (0x70a6_4bea, 67, eva::event),
        "EvaSettings" => (0xafad_556c, 68, eva::settings),
        "LocomotorTemplate" => (0xca32_f95d, 69, locomotor::locomotor_template),
        "BounceKickTerrainMapping" => (0x7dc1_f449, 70, locomotor::bounce_kick_terrain_mapping),
        "TerrainParticleMapping" => (0xa321_84fe, 71, surface_mappings::terrain_particles),
        "DynamicsContactTagMapping" => (0x358a_b09d, 72, surface_mappings::dynamics_contacts),
        "PlayerPowerButtonTemplate" => (0xa640_9dd2, 73, player_power_button::compile),
        "JoypadCommandBarButtonTemplate" => (0x2913_cc3c, 74, joypad_buttons::compile),
        "StanceTemplate" => (0x4829_f362, 75, stance_template::compile),
        "GameScriptList" => (0xa268_995a, 76, game_script_list::compile),
        "Road" => (0x2bfd_2964, 77, road::compile),
        "GameMap" => (0xf465_7c12, 78, game_map::compile),
        "AudioSettings" => (0xd86e_d334, 83, audio_settings::settings),
        "AnimationSoundSettings" => (0xbcf8_03d4, 83, audio_settings::animation),
        "GraphicsHardware" => (0x42aa_52a8, 82, game_lod::hardware),
        "GameLODPreset" => (0xd948_d68d, 83, game_lod::preset),
        "StaticGameLOD" => (0x4a93_fc61, 83, game_lod::static_lod),
        "DynamicGameLOD" => (0x692b_85b7, 83, game_lod::dynamic_lod),
        "AudioLOD" => (0x014c_bb53, 83, audio_settings::lod),
        "MusicScriptTrack" => (0xbbde_2f64, 79, music::script_track),
        "MusicPalette" => (0xd96f_c160, 80, music::palette),
        "W3DAnimation" => (0x5545_5fe8, 66, w3d_animation::compile),
        "W3DMesh" => (0xcd8a_089c, 35, w3d_mesh::compile),
        "W3DContainer" => (0x619d_a33f, 36, w3d_mesh::container),
        "AmbientStream" => (0x6d16_24fb, 37, audio::single_sound),
        "AudioEvent" => (0x560c_2e45, 38, audio::single_sound),
        "MusicTrack" => (0x07c5_0662, 59, audio::single_sound),
        "DialogEvent" => (0xd21e_8bf8, 60, audio::single_sound),
        "AudioEventLimitGroup" => (0x9531_4dea, 61, audio::limit_group),
        "Multisound" => (0xf79c_5a89, 57, audio::multisound),
        "FXParticleSystemTemplate" => (0x575c_23ba, 58, particle_system::compile),
        "OnDemandTextureImage" => (0x13db_2fbc, 41, on_demand_image),
        "DamageFX" => (0x0b2e_b0ab, 42, damage_fx::compile),
        "EmotionTemplate" => (0x40da_32d1, 54, emotion::compile),
        "ObjectCreationList" => (0x2273_a9d1, 55, object_creation::compile),
        "ObjectFilterAsset" => (0x4352_195e, 43, game_object::object_filter_asset),
        "PlayerTemplate" => (0x3521_4948, 50, player_template::compile),
        "PlayerTech" => (0x35dd_9987, 44, player_tech),
        "PlayerTechStoreTemplate" => (0x3a84_e115, 45, player_tech_store),
        "SpecialPowerTemplate" => (0x3aab_481c, 46, game_object::special_power::compile),
        "WeaponTemplate" => (0x0ce6_168e, 48, weapon::compile),
        "FXList" => (0x9b13_1e26, 56, fx_list::compile),
        "GameObject" => (0xcb8d_0980, 10, game_object::compile),
        "PathMusicGameDynamicState" => (0xe2e2_4da7, 80, music::dynamic_state),
        "PathMusicGameDynamicStateSet" => (0x511d_9d37, 81, music::dynamic_state_set),
        "PathMusicGameDynamicTransition" => (0x82f0_728e, 79, music::dynamic_transition),
        "PathMusicEventSet" => (0x6209_4424, 23, music::event_set),
        "MapMetaData" => (0x481d_a8e6, 24, campaign::map_metadata),
        "MissionTemplate" => (0xd299_ec3a, 25, campaign::mission),
        "CampaignTemplate" => (0xd0c4_5a63, 26, campaign::campaign),
        "CampaignEnumerator" => (0x0714_c282, 27, campaign_enumerator),
        _ => (
            music_conditions::type_hash(type_name)?,
            78,
            music_conditions::compile,
        ),
    };
    Some(encoder)
}

pub(super) fn has_encoder(type_name: &str) -> bool {
    encoder(type_name).is_some()
        || matches!(
            type_name,
            "PathMusicEvent"
                | "PathMusicMap"
                | "PathMusicTrack"
                | "AudioFile"
                | "OnDemandTexture"
                | "Texture"
                | "TerrainTextureAtlas"
        )
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
    let processing_hash = if type_name == "W3DAnimation" {
        0xd650_5ce9
    } else if type_name == "W3DMesh" {
        // RA3's render plugin uses its compiler version XOR the runtime type hash.
        type_hash ^ 8
    } else if data.tokenized {
        type_hash + 0x1004
    } else {
        type_hash ^ 4
    };
    let mut instance_hash = hash_xml(node, processing_hash, path)?;
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
        weak_references: Vec::new(),
        custom_data: None,
        tokenized: data.tokenized,
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
    tokenized: bool,
}

pub(super) fn validate_attributes(
    node: Node<'_, '_>,
    allowed: &[&str],
    path: &Path,
) -> io::Result<()> {
    for attribute in node.attributes() {
        if attribute.namespace() == Some(super::XML_NS) && attribute.name() == "base" {
            continue;
        }
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
    let filename = node
        .attribute("Filename")
        .filter(|value| {
            value
                .bytes()
                .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        })
        .ok_or_else(|| invalid(path, "expected an ASCII Video Filename attribute"))?;
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

fn production_queue_type(value: &str, field: &str, path: &Path) -> io::Result<u32> {
    const QUEUES: &[&str] = &[
        "INVALID",
        "MAIN_STRUCTURE",
        "OTHER_STRUCTURE",
        "INFANTRY",
        "VEHICLE",
        "AIRCRAFT",
        "UPGRADE",
        "WATERCRAFT",
    ];
    QUEUES
        .iter()
        .position(|&queue| queue == value)
        .map(|index| (index as u32).wrapping_sub(1))
        .ok_or_else(|| invalid(path, format!("invalid {field} {value}")))
}

fn sage_uint(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    let value = value.trim();
    let number = if value == "0.0" {
        Ok(0)
    } else if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse::<u64>()
    };
    number
        .map(|value| value.min(u64::from(u32::MAX)) as u32)
        .map_err(|_| invalid(path, format!("invalid {name} {value:?}")))
}

fn sage_int(value: &str, name: &str, path: &Path) -> io::Result<u32> {
    let value = value.trim();
    let number = if let Some(hex) = value.strip_prefix("0x") {
        i64::from_str_radix(hex, 16)
    } else {
        value.parse::<i64>()
    };
    number
        .map(|value| value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32 as u32)
        .map_err(|_| invalid(path, format!("invalid {name} {value:?}")))
}

fn finite_number(value: &str, field: &str, path: &Path) -> io::Result<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or_else(|| invalid(path, format!("invalid {field}")))
}

fn sage_angle(value: &str, field: &str, path: &Path) -> io::Result<f32> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix('d') {
        Ok(finite_number(number, field, path)?.to_radians())
    } else if let Some(number) = value.strip_suffix('r') {
        finite_number(number, field, path)
    } else {
        Err(invalid(path, format!("invalid {field}")))
    }
}

fn sage_time(value: &str, field: &str, path: &Path) -> io::Result<f32> {
    if let Some(number) = value.trim().strip_suffix("ms") {
        return Ok(finite_number(number, field, path)? * 0.001);
    }
    let number = value
        .trim()
        .strip_suffix('s')
        .ok_or_else(|| invalid(path, format!("invalid {field}")))?;
    finite_number(number, field, path)
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

pub(super) fn significant_children<'a, 'input>(
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
    color(node, &["B", "G", "R", "A"], 0, path)
}

fn color(node: Node<'_, '_>, channels: &[&str], alpha: u8, path: &Path) -> io::Result<u32> {
    validate_attributes(node, channels, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in color"));
    }
    let mut bytes = [0, 0, 0, alpha];
    for (byte, &channel) in bytes.iter_mut().zip(channels) {
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

fn on_demand_image(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Texture", "Rotated"], path)?;
    let mut words = [0; 9];
    let mut result = Marshaled::default();
    if let Some(texture) = node
        .attribute("Texture")
        .filter(|value| !value.trim().is_empty())
    {
        let texture = texture.split_once('\\').map_or(texture, |(name, _)| name);
        let id = asset_reference(texture, "OnDemandTexture", path)?;
        result.references.push(id.into());
        result.reference_types.push(id.type_id);
        result.imports.extend([4, u32::MAX]);
        words[1] = 1;
    }
    words[8] = match node.attribute("Rotated").unwrap_or("false").trim() {
        "true" | "1" => 1,
        "false" | "0" => 0,
        _ => return Err(invalid(path, "invalid Rotated")),
    };
    let mut children = significant_children(node);
    for (index, name) in ["Dimensions", "Coords", "TextureDimensions"]
        .into_iter()
        .enumerate()
    {
        let child = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, name)) && !has_content(*child))
            .ok_or_else(|| invalid(path, format!("expected {name}")))?;
        validate_attributes(child, &["x", "y"], path)?;
        for (axis, name) in ["x", "y"].into_iter().enumerate() {
            words[2 + index * 2 + axis] = child
                .attribute(name)
                .unwrap_or("0")
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid image coordinate {name}")))?
                as u32;
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected OnDemandTextureImage child"));
    }
    result.payload = encode_words(words);
    Ok(result)
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
        ..Marshaled::default()
    })
}

fn player_tech_store(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Faction"], path)?;
    let faction = global_ui::enum_value(
        required(node, "Faction", path)?,
        &["RANDOM", "Allies", "Soviet", "Japan", "Neutral"],
        path,
    )?
    .wrapping_sub(1);
    let rows: Vec<_> = significant_children(node).collect();
    if rows.len() != 5 {
        return Err(invalid(path, "expected five tech store rows"));
    }
    let mut data = binary::BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.word(4, faction);
    let first = data.list(8, rows.len(), 8)?;
    for (index, row) in rows.into_iter().enumerate() {
        validate_attributes(row, &[], path)?;
        let buttons: Vec<_> = significant_children(row).collect();
        if !row.has_tag_name((SAGE_NS, "Row")) || buttons.len() != 3 {
            return Err(invalid(
                path,
                "expected three buttons in each tech store row",
            ));
        }
        let start = data.list(first + index * 8, buttons.len(), 4)?;
        for (index, button) in buttons.into_iter().enumerate() {
            if !button.has_tag_name((SAGE_NS, "Button")) {
                return Err(invalid(path, "expected tech store Button"));
            }
            let id = asset_reference(
                text_value(button, path)?,
                "PurchasePlayerTechButtonTemplate",
                path,
            )?;
            result.reference_types.push(id.type_id);
            result.references.push(id.into());
            let offset = start + index * 4;
            data.word(offset, result.references.len() as u32);
            result.imports.push(offset as u32);
        }
    }
    data.relocations.push(u32::MAX);
    result.imports.push(u32::MAX);
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

fn player_tech(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "TechPointsRequired"], path)?;
    let points = node
        .attribute("TechPointsRequired")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid TechPointsRequired"))?;
    reference_list(node, path, "TechDependency", "PlayerTech", vec![0, points])
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

fn logic_command(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    use std::sync::OnceLock;
    static ENUMS: OnceLock<[Vec<String>; 2]> = OnceLock::new();
    let enums = ENUMS.get_or_init(|| {
        let schema = roxmltree::Document::parse(include_str!(
            "../../schemas/ra3/xsd/AssetTypeLogicCommand.xsd"
        ))
        .unwrap();
        ["LogicCommandType", "LogicCommandOptions"].map(|name| {
            schema
                .descendants()
                .find(|node| node.attribute("name") == Some(name))
                .unwrap()
                .descendants()
                .filter_map(|node| node.attribute("value").map(str::to_owned))
                .collect()
        })
    });
    validate_attributes(node, &["id", "Type", "Options"], path)?;
    let kind = node.attribute("Type").unwrap_or("NONE");
    let ordinal = enums[0]
        .iter()
        .position(|v| v == kind)
        .ok_or_else(|| invalid(path, "invalid LogicCommand Type"))?;
    let mut flags = 0;
    for option in node
        .attribute("Options")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        let bit = enums[1]
            .iter()
            .position(|v| v == option)
            .filter(|&i| i < 32)
            .ok_or_else(|| invalid(path, format!("invalid LogicCommand option {option}")))?;
        flags |= 1 << bit;
    }
    let mut data = binary::BinaryData {
        payload: vec![0; 28],
        relocations: Vec::new(),
        path,
    };
    data.word(4, ordinal as u32);
    data.word(8, flags);
    let mut result = Marshaled::default();
    let mut children = significant_children(node).peekable();
    for (name, ty, field) in [
        ("SpecialPower", "SpecialPowerTemplate", 12),
        ("Upgrade", "UpgradeTemplate", 16),
        ("Object", "GameObject", 20),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            let value = text_value(child, path)?;
            let offset = data.allocate(field, 4)?;
            write_import(value, ty, offset, &mut data, &mut result)?;
        }
    }
    if let Some(info) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "AISpecialPowerInfo")))
    {
        ai_command::write_info(info, &mut data, &mut result)?;
    }
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!("unsupported LogicCommand child {}", child.tag_name().name()),
        ));
    }
    if !data.relocations.is_empty() {
        data.relocations.sort_unstable();
        data.relocations.push(u32::MAX);
    }
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
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
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
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
        ..Marshaled::default()
    })
}

fn write_import(
    value: &str,
    ty: &str,
    offset: usize,
    data: &mut binary::BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let ordinal = if ty == "BaseAudioEventInfo" {
        game_object::audio_reference(
            value,
            data.path,
            &mut result.references,
            &mut result.reference_types,
        )?
    } else {
        let target = value.trim().split_once(':').map_or(ty, |(kind, _)| kind);
        let id = if ty == music_conditions::BASE && music_conditions::type_hash(target).is_some() {
            asset_reference(value, target, data.path)?
        } else {
            asset_reference(value, ty, data.path)?
        };
        result.reference_types.push(fast_hash(ty.as_bytes()));
        result.references.push(id.into());
        result.references.len() as u32
    };
    if ordinal == 0 {
        return Ok(());
    }
    data.word(offset, ordinal);
    result.imports.push(offset as u32);
    Ok(())
}

fn asset_reference(value: &str, target_type: &str, path: &Path) -> io::Result<AssetId> {
    let value = value.trim();
    // Inherited XML can carry a reference ordinal appended by the schema materializer.
    let value = value.split_once('\\').map_or(value, |(name, _)| name);
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

fn schema_enums<const N: usize>(schema: &'static str, names: [&str; N]) -> [Vec<String>; N] {
    let document = roxmltree::Document::parse(schema).expect("bundled schema must be valid");
    names.map(|name| {
        document
            .descendants()
            .find(|node| node.attribute("name") == Some(name))
            .expect("bundled enum must exist")
            .descendants()
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    })
}
