use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, hash_name, significant_children, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const SCRIPTED_MODEL_DRAW: u32 = 0x4ecf_2a4b;
const MODEL_STATE: u32 = 0x953c_f093;
const STRING: u32 = 0x1a29_33aa;
const FLOAT: u32 = 0x6458_64f3;
const BOOL: u32 = 0x77e3_f4f7;

pub(in crate::compile) const SCRIPTED_FLAGS: &[(&str, usize, bool)] = &[
    ("OkToChangeModelColor", 0xc4, false),
    ("AnimationsRequirePower", 0xc5, true),
    ("UseYAxisForTurretRotation", 0xc6, false),
    ("TrackMarksOnlyWhenCorneringQuickly", 0xc7, false),
    ("UseProducerTexture", 0xc8, false),
    ("NoRotate", 0xc9, false),
    ("UseFiringArcRotation", 0xca, false),
    ("Selectable", 0xcb, true),
    ("RandomTextureFixedRandomIndex", 0xcc, false),
    ("ParticleBonesCheckDrawable", 0xcd, false),
    ("ShadowForceDisable", 0xce, false),
    ("SwitchModelLODMode", 0xcf, false),
    ("StaticModelLODMode", 0xd0, false),
    ("ShowShadowWhileContained", 0xd1, false),
    ("MultiPlayerOnly", 0xd2, false),
    ("AffectedByStealth", 0xd3, true),
    ("InvertStealthOpacity", 0xd4, false),
    ("HighDetailOnly", 0xd5, false),
];

fn scripted_flag(name: &'static str) -> (&'static str, u32, usize) {
    let &(_, offset, _) = SCRIPTED_FLAGS
        .iter()
        .find(|&&(field, _, _)| field == name)
        .expect("ScriptedModelDraw flag in layout");
    (name, BOOL, offset)
}

pub(super) fn encode(
    group: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
) -> io::Result<Target> {
    let modules: Vec<_> = significant_children(group).collect();
    let list = data.allocate(0x1cc, 8)?;
    let pointers = data.list(list, modules.len(), 4)?;
    let mut items = Vec::with_capacity(modules.len());
    for (index, node) in modules.into_iter().enumerate() {
        let pointer = pointers + index * 4;
        let (kind, module) = match node.tag_name().name() {
            "ScriptedModelDraw" if node.has_tag_name((SAGE_NS, "ScriptedModelDraw")) => (
                SCRIPTED_MODEL_DRAW,
                scripted_model(
                    node,
                    pointer,
                    data,
                    references,
                    reference_types,
                    material_float_names,
                )?,
            ),
            "LaserDraw" if node.has_tag_name((SAGE_NS, "LaserDraw")) => (
                super::laser_draw::KIND,
                super::laser_draw::encode(
                    node,
                    pointer,
                    data,
                    references,
                    reference_types,
                    material_float_names,
                )?,
            ),
            name => {
                return Err(invalid(
                    data.path,
                    format!("unsupported Draws module {name}"),
                ));
            }
        };
        items.push((
            0,
            Target::Table(Table::sequence(vec![Entry::new(
                2,
                kind,
                Target::Table(module),
            )])),
        ));
    }
    Ok(Target::Table(Table::array(list, items)))
}

const SCRIPTED_STRINGS: [(&str, usize); 4] = [
    ("Name", 8),
    ("AttachToBoneInAnotherModule", 0x34),
    ("TrackMarksLeftBone", 0x78),
    ("TrackMarksRightBone", 0x80),
];
const SCRIPTED_REALS: [(&str, usize); 9] = [
    ("InitialRecoilSpeed", 0x10),
    ("MaxRecoilDistance", 0x14),
    ("RecoilDamping", 0x18),
    ("RecoilSettleSpeed", 0x1c),
    ("HighDetailLODThreshold", 0x88),
    ("LowDetailLODThreshold", 0x8c),
    ("AlphaCameraFadeOuterRadius", 0x94),
    ("AlphaCameraFadeInnerRadius", 0x98),
    ("AlphaCameraAtInnerRadius", 0x9c),
];

fn scripted_model(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    let allowed: Vec<_> = [
        "id",
        "ExtraPublicBone",
        "DependencySharedModelFlags",
        "BirthFadeTime",
    ]
    .into_iter()
    .chain(SCRIPTED_FLAGS.iter().map(|&(name, _, _)| name))
    .chain(
        SCRIPTED_STRINGS
            .into_iter()
            .chain(SCRIPTED_REALS)
            .map(|(name, _)| name),
    )
    .collect();
    validate_attributes(node, &allowed, data.path)?;
    let mut states = Vec::new();
    let mut animations = Vec::new();
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "ModelConditionState")) {
            states.push(child);
        } else if child.has_tag_name((SAGE_NS, "AnimationState")) {
            animations.push(child);
        } else {
            return Err(invalid(
                data.path,
                format!(
                    "unsupported ScriptedModelDraw child {}",
                    child.tag_name().name()
                ),
            ));
        }
    }

    let start = data.allocate(pointer, 0xd8)?;
    data.word(start, SCRIPTED_MODEL_DRAW);
    data.word(start + 4, hash_name(required(node, "id", data.path)?));
    for (offset, bits) in [
        (0x10, 0x3ecc_cccd),
        (0x14, 0x4040_0000),
        (0x18, 0x3ecc_cccd),
        (0x1c, 0x3c54_fdf3),
        (0x9c, 0x3f80_0000),
    ] {
        data.word(start + offset, bits);
    }
    for (name, offset) in SCRIPTED_STRINGS {
        if let Some(value) = node.attribute(name) {
            data.string(start + offset, value)?;
        }
    }
    for (name, offset) in SCRIPTED_REALS {
        if let Some(value) = node.attribute(name) {
            let value = match name {
                "AlphaCameraAtInnerRadius" => super::super::percentage(value, name, data.path)?,
                "InitialRecoilSpeed" | "RecoilSettleSpeed" => {
                    super::super::finite_number(value, name, data.path)? * 0.2
                }
                _ => super::super::finite_number(value, name, data.path)?,
            };
            data.word(start + offset, value.to_bits());
        }
    }
    if let Some(value) = node.attribute("DependencySharedModelFlags") {
        write_model_conditions(value, start + 0x3c, data)?;
    }
    if let Some(value) = node.attribute("BirthFadeTime") {
        let value = value
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(data.path, "invalid BirthFadeTime"))?;
        data.word(start + 0xa0, value as u32);
    }
    for &(name, offset, default) in SCRIPTED_FLAGS {
        data.payload[start + offset] = boolean(node, name, default, data.path)?;
    }

    let bones = node
        .attribute("ExtraPublicBone")
        .unwrap_or("")
        .split_ascii_whitespace();
    let bone_offset = start + 0x2c;
    let bone_data = data.list(bone_offset, bones.clone().count(), 8)?;
    let mut bone_tokens = Vec::new();
    for (index, name) in bones.enumerate() {
        let offset = bone_data + index * 8;
        data.string(offset, name)?;
        bone_tokens.push((0, Target::Data(offset)));
    }

    let state_offset = start + 0xa4;
    let state_data = data.list(state_offset, states.len(), 0xc4)?;
    let mut state_tables = Vec::with_capacity(states.len());
    for (index, state) in states.into_iter().enumerate() {
        let fields = model_state(
            state,
            state_data + index * 0xc4,
            data,
            references,
            reference_types,
            material_float_names,
        )?;
        state_tables.push((0, Target::Table(fields)));
    }
    let animation_table =
        super::animation::encode(animations, start + 0xac, data, references, reference_types)?;

    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let base = Table::record(vec![Entry::new(1, 0xd75d_93a2, Target::Table(module))]);
    let mut entries = vec![Entry::new(1, 0x17f6_1728, Target::Table(base))];
    entries.extend(fields(
        start,
        &[
            ("Name", STRING, 0x08),
            ("InitialRecoilSpeed", 0x7a76_c60e, 0x10),
            ("MaxRecoilDistance", FLOAT, 0x14),
            ("RecoilDamping", FLOAT, 0x18),
            ("RecoilSettleSpeed", 0x7a76_c60e, 0x1c),
            scripted_flag("OkToChangeModelColor"),
            scripted_flag("AnimationsRequirePower"),
            scripted_flag("UseYAxisForTurretRotation"),
            ("MinLODRequired", 0xa80d_9f50, 0x20),
            ("ProjectileBoneFeedbackEnabledSlots", 0x28ae_db15, 0x24),
            ("TrackMarks", 0x4c3e_3938, 0x28),
            scripted_flag("TrackMarksOnlyWhenCorneringQuickly"),
        ],
    ));
    entries.push(Entry::named(
        "ExtraPublicBone",
        0x6d66_ae18,
        Target::Table(Table::array(bone_offset, bone_tokens)),
    ));
    entries.extend(fields(
        start,
        &[
            ("AttachToBoneInAnotherModule", STRING, 0x34),
            ("DependencySharedModelFlags", 0x67db_bfc9, 0x3c),
            scripted_flag("UseProducerTexture"),
            scripted_flag("NoRotate"),
            scripted_flag("UseFiringArcRotation"),
            scripted_flag("Selectable"),
            scripted_flag("RandomTextureFixedRandomIndex"),
            ("TrackMarksLeftBone", STRING, 0x78),
            ("TrackMarksRightBone", STRING, 0x80),
            scripted_flag("ParticleBonesCheckDrawable"),
            scripted_flag("ShadowForceDisable"),
            ("HighDetailLODThreshold", FLOAT, 0x88),
            ("LowDetailLODThreshold", FLOAT, 0x8c),
            scripted_flag("SwitchModelLODMode"),
            scripted_flag("StaticModelLODMode"),
            scripted_flag("ShowShadowWhileContained"),
            ("WadingParticleSys", 0x43f1_ce36, 0x90),
            ("AlphaCameraFadeOuterRadius", FLOAT, 0x94),
            ("AlphaCameraFadeInnerRadius", FLOAT, 0x98),
            ("AlphaCameraAtInnerRadius", FLOAT, 0x9c),
            ("BirthFadeTime", 0x23e6_50b8, 0xa0),
            scripted_flag("MultiPlayerOnly"),
            scripted_flag("AffectedByStealth"),
            scripted_flag("InvertStealthOpacity"),
            scripted_flag("HighDetailOnly"),
        ],
    ));
    entries.extend([
        Entry::named(
            "ModelConditionState",
            MODEL_STATE,
            Target::Table(Table::array(state_offset, state_tables)),
        ),
        Entry::named(
            "AnimationState",
            0x99b3_42c4,
            Target::Table(animation_table),
        ),
        array("AttachModel", 0xb4e5_23bf, start + 0xb4),
        array("EmbedPortal", 0xafc5_e3e3, start + 0xbc),
    ]);
    Ok(Table::record(entries))
}

const WEAPON_BONE_LISTS: [(&str, usize); 4] = [
    ("WeaponFireFXBone", 0x7c),
    ("WeaponRecoilBone", 0x84),
    ("WeaponMuzzleFlash", 0x8c),
    ("WeaponLaunchBone", 0x94),
];

fn model_state(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    validate_attributes(
        node,
        &["ParseCondStateType", "ConditionsYes", "RetainSubObjects"],
        data.path,
    )?;
    let parse_type = match required(node, "ParseCondStateType", data.path)? {
        "PARSE_NORMAL" => 0,
        "PARSE_DEFAULT" => 1,
        "PARSE_TRANSITION" => 2,
        value => {
            return Err(invalid(
                data.path,
                format!("invalid ParseCondStateType {value}"),
            ));
        }
    };
    data.word(start, parse_type);
    data.payload[start + 0xc0] = boolean(node, "RetainSubObjects", false, data.path)?;
    if let Some(value) = node.attribute("ConditionsYes") {
        write_model_conditions(value, start + 0xc, data)?;
    }
    let mut weapon_bones: [Vec<_>; 4] = std::array::from_fn(|_| Vec::new());
    let mut models = Vec::new();
    let mut material = None;
    let mut textures = Vec::new();
    let mut particles = Vec::new();
    let mut turrets = Vec::new();
    let mut shadow = None;
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "Model")) {
            models.push(child);
        } else if child.has_tag_name((SAGE_NS, "Material")) && material.is_none() {
            material = Some(child);
        } else if child.has_tag_name((SAGE_NS, "Texture")) {
            textures.push(child);
        } else if child.tag_name().namespace() == Some(SAGE_NS)
            && let Some(index) = WEAPON_BONE_LISTS
                .iter()
                .position(|&(name, _)| child.tag_name().name() == name)
        {
            weapon_bones[index].push(child);
        } else if child.has_tag_name((SAGE_NS, "ParticleSysBone")) {
            particles.push(child);
        } else if child.has_tag_name((SAGE_NS, "ShadowInfo")) && shadow.is_none() {
            shadow = Some(child);
        } else if child.has_tag_name((SAGE_NS, "Turret")) {
            turrets.push(child);
        } else {
            return Err(invalid(
                data.path,
                format!(
                    "unsupported or repeated ModelConditionState child {}",
                    child.tag_name().name()
                ),
            ));
        }
    }
    let model_offset = start + 0x68;
    let model_data = data.list(model_offset, models.len(), 8)?;
    let mut model_tables = Vec::with_capacity(models.len());
    for (index, model) in models.into_iter().enumerate() {
        validate_attributes(model, &["Name", "ExtraMesh"], data.path)?;
        if has_content(model) {
            return Err(invalid(data.path, "Model cannot contain content"));
        }
        let offset = model_data + index * 8;
        if let Some(name) = model.attribute("Name").filter(|name| !name.is_empty()) {
            let id = asset_reference(name, "BaseRenderAssetType", data.path)?;
            references.push(id.into());
            reference_types.push(id.type_id);
            data.word(offset, references.len() as u32);
        }
        data.payload[offset + 4] = boolean(model, "ExtraMesh", false, data.path)?;
        model_tables.push((
            0,
            Target::Table(Table::record(
                fields(offset, &[("Name", 0xfcb1_06d5, 0), ("ExtraMesh", BOOL, 4)]).collect(),
            )),
        ));
    }
    let material = match material {
        Some(node) => Target::Table(super::material::encode(
            node,
            start + 0x70,
            data,
            references,
            reference_types,
            material_float_names,
        )?),
        None => Target::Null,
    };
    let textures = super::subobjects::encode_texture_replacements(
        textures,
        start + 0x74,
        data,
        references,
        reference_types,
    )?;
    let mut bone_entries = Vec::with_capacity(WEAPON_BONE_LISTS.len());
    for ((name, offset), bones) in WEAPON_BONE_LISTS.into_iter().zip(weapon_bones) {
        let table = encode_weapon_bones(bones, start + offset, data)?;
        bone_entries.push(Entry::named(name, 0xbd39_d1a1, Target::Table(table)));
    }
    let particle_table = super::animation::encode_particles(
        particles,
        start + 0x9c,
        data,
        references,
        reference_types,
    )?;
    let shadow = match shadow {
        Some(node) => Target::Table(super::shadow::encode(
            node,
            start + 0xac,
            data,
            references,
            reference_types,
        )?),
        None => Target::Null,
    };
    let mut entries: Vec<_> = fields(
        start,
        &[
            ("ParseCondStateType", 0x1323_b870, 0),
            ("Name", STRING, 0x04),
            ("ConditionsYes", 0x67db_bfc9, 0x0c),
            ("Skeleton", 0x805d_25f4, 0x48),
            ("PortraitImage", 0x0108_bd1e, 0x50),
            ("ButtonImage", 0x0108_bd1e, 0x54),
            ("OverrideTooltip", STRING, 0x58),
            ("RetainSubObjects", BOOL, 0xc0),
            ("id", 0x805d_25f4, 0x60),
        ],
    )
    .collect();
    entries.extend([
        Entry::named(
            "Model",
            0xcd8e_c40b,
            Target::Table(Table::array(model_offset, model_tables)),
        ),
        Entry::named("Material", 0xc7ea_3cf4, material),
        Entry::named("Texture", 0x6e5d_40fb, Target::Table(textures)),
    ]);
    entries.extend(bone_entries);
    entries.extend([
        Entry::named(
            "ParticleSysBone",
            0x98c5_3fe6,
            Target::Table(particle_table),
        ),
        array("FXEvent", 0x7599_1ad3, start + 0xa4),
        Entry::named("ShadowInfo", 0x82fe_7e88, shadow),
        Entry::named(
            "Turret",
            0xf309_5903,
            Target::Table(encode_turrets(turrets, start + 0xb0, data)?),
        ),
        array("SubObject", 0x03a2_8c40, start + 0xb8),
    ]);
    Ok(Table::record(entries))
}

fn encode_turrets(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    const FIELDS: &[(&str, u32, usize)] = &[
        ("TurretNameKey", STRING, 0),
        ("TurretArtAngle", 0x4e16_ea4d, 8),
        ("TurretPitch", STRING, 12),
        ("TurretArtPitch", 0x4e16_ea4d, 20),
        ("TurretID", 0x23e6_50b8, 24),
    ];
    let path = data.path;
    let allowed: Vec<_> = FIELDS.iter().map(|&(name, _, _)| name).collect();
    let start = data.list(count, nodes.len(), 0x1c)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &allowed, path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected model-state Turret content"));
        }
        let offset = start + index * 0x1c;
        for &(name, kind, field) in FIELDS {
            let value = node.attribute(name);
            match kind {
                STRING => {
                    if let Some(value) = value {
                        data.string(offset + field, value)?;
                    }
                }
                0x4e16_ea4d => {
                    let value = super::slow_death::angle(value.unwrap_or("0r"), name, data)?;
                    data.word(offset + field, value.to_bits());
                }
                _ => {
                    let id = value
                        .unwrap_or("-1")
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| invalid(path, "invalid TurretID"))?;
                    data.word(offset + field, id as u32);
                }
            }
        }
        items.push((
            0,
            Target::Table(Table::record(fields(offset, FIELDS).collect())),
        ));
    }
    Ok(Table::array(count, items))
}

fn encode_weapon_bones(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    const SLOTS: &[&str] = &[
        "PRIMARY_WEAPON",
        "SECONDARY_WEAPON",
        "TERTIARY_WEAPON",
        "QUATERNARY_WEAPON",
        "QUINARY_WEAPON",
        "NO_WEAPON",
    ];
    let path = data.path;
    let start = data.list(count, nodes.len(), 0x10)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &["WeaponSlotType", "BoneName"], path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected weapon bone content"));
        }
        let offset = start + index * 0x10;
        let slot = super::super::global_ui::enum_value(
            node.attribute("WeaponSlotType").unwrap_or("PRIMARY_WEAPON"),
            SLOTS,
            path,
        )?;
        data.word(offset, 1);
        data.word(offset + 4, slot);
        if let Some(name) = node.attribute("BoneName") {
            data.string(offset + 8, name)?;
        }
        let mut entries = vec![Entry::new(0x5e25_9cb2, 0x23e6_50b8, Target::Data(offset))];
        entries.extend(fields(
            offset,
            &[("WeaponSlotType", 0x1848_c549, 4), ("BoneName", STRING, 8)],
        ));
        items.push((0, Target::Table(Table::record(entries))));
    }
    Ok(Table::array(count, items))
}

pub(super) fn fields<'a>(
    start: usize,
    fields: &'a [(&'a str, u32, usize)],
) -> impl Iterator<Item = Entry> + 'a {
    fields
        .iter()
        .map(move |&(name, kind, offset)| Entry::named(name, kind, Target::Data(start + offset)))
}

pub(super) fn array(name: &str, kind: u32, count: usize) -> Entry {
    Entry::named(name, kind, Target::Table(Table::array(count, [])))
}

pub(super) fn boolean(
    node: Node<'_, '_>,
    name: &str,
    default: bool,
    path: &std::path::Path,
) -> io::Result<u8> {
    match node.attribute(name) {
        None => Ok(u8::from(default)),
        Some("true" | "1") => Ok(1),
        Some("false" | "0") => Ok(0),
        Some(value) => Err(invalid(path, format!("invalid {name} {value}"))),
    }
}

pub(super) fn write_model_conditions(
    value: &str,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for flag in value.split_ascii_whitespace() {
        let index = model_condition_index(flag, data.path)?;
        data.payload[start + index / 8] |= 1 << (index % 8);
    }
    Ok(())
}

pub(super) fn model_condition_index(flag: &str, path: &std::path::Path) -> io::Result<usize> {
    static FLAGS: OnceLock<Vec<String>> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../schemas/ra3/xsd/Includes/ModelState.xsd"
        ))
        .unwrap();
        document
            .descendants()
            .find(|node| node.attribute("name") == Some("ModelConditionFlagType"))
            .unwrap()
            .descendants()
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .filter(|name| name != "INVALID")
            .collect()
    });
    flags
        .iter()
        .take(0x3c * 8)
        .position(|name| name == flag)
        .ok_or_else(|| invalid(path, format!("invalid model condition {flag}")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_condition_particles_match_ea() {
        use crate::compile::static_assets::compile_asset;
        use crate::manifest::Manifest;
        use std::path::Path;

        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-model-particles.xml"
        ))
        .unwrap();
        let node = document.root_element().first_element_child().unwrap();
        let actual = compile_asset(node, Path::new("game-object-model-particles.xml"), "").unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-model-particles.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-model-particles.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-model-particles.manifest"
        ))
        .unwrap();
        assert_eq!(actual.payload.len(), bin.len() - 4);
        assert_eq!(
            actual
                .payload
                .iter()
                .zip(&bin[4..])
                .position(|(a, b)| a != b),
            None
        );
        assert_eq!(actual.relocations, relo[4..]);
        assert!(actual.imports.is_empty());
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.assets[0].references.iter())
        );
    }

    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn scripted_model_draw_matches_ea() {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-draw-minimal.xml"
        ))
        .unwrap();
        let node = document.root_element().first_element_child().unwrap();
        let actual = compile_asset(node, Path::new("draw.xml"), "DATA:draw.xml").unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-draw-minimal.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-draw-minimal.relo");
        let oracle = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-draw-minimal.manifest"
        ))
        .unwrap();
        assert_eq!(actual.payload.len(), bin.len() - 4);
        assert_eq!(
            actual
                .payload
                .iter()
                .zip(&bin[4..])
                .position(|(a, b)| a != b),
            None,
            "first mismatched payload byte"
        );
        assert_eq!(actual.relocations, relo[4..]);
        assert!(actual.imports.is_empty());
        assert_eq!(actual.instance_hash, oracle.assets[0].instance_hash);
        assert_eq!(actual.references.len(), 1);
        assert_eq!(
            actual.references[0].id,
            oracle.assets[0].references.iter().next().unwrap()
        );
    }
}
