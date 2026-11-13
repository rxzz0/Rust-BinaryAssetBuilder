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
        if !node.has_tag_name((SAGE_NS, "ScriptedModelDraw")) {
            return Err(invalid(
                data.path,
                format!("unsupported Draws module {}", node.tag_name().name()),
            ));
        }
        let module = scripted_model(
            node,
            pointers + index * 4,
            data,
            references,
            reference_types,
            material_float_names,
        )?;
        items.push((
            0,
            Target::Table(Table::sequence(vec![Entry::new(
                2,
                SCRIPTED_MODEL_DRAW,
                Target::Table(module),
            )])),
        ));
    }
    Ok(Target::Table(Table::array(list, items)))
}

fn scripted_model(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    validate_attributes(
        node,
        &[
            "id",
            "OkToChangeModelColor",
            "Selectable",
            "ExtraPublicBone",
        ],
        data.path,
    )?;
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
    data.payload[start + 0xc4] = boolean(node, "OkToChangeModelColor", false, data.path)?;
    data.payload[start + 0xc5] = 1;
    data.payload[start + 0xcb] = boolean(node, "Selectable", true, data.path)?;
    data.payload[start + 0xd3] = 1;

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
            ("OkToChangeModelColor", BOOL, 0xc4),
            ("AnimationsRequirePower", BOOL, 0xc5),
            ("UseYAxisForTurretRotation", BOOL, 0xc6),
            ("MinLODRequired", 0xa80d_9f50, 0x20),
            ("ProjectileBoneFeedbackEnabledSlots", 0x28ae_db15, 0x24),
            ("TrackMarks", 0x4c3e_3938, 0x28),
            ("TrackMarksOnlyWhenCorneringQuickly", BOOL, 0xc7),
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
            ("UseProducerTexture", BOOL, 0xc8),
            ("NoRotate", BOOL, 0xc9),
            ("UseFiringArcRotation", BOOL, 0xca),
            ("Selectable", BOOL, 0xcb),
            ("RandomTextureFixedRandomIndex", BOOL, 0xcc),
            ("TrackMarksLeftBone", STRING, 0x78),
            ("TrackMarksRightBone", STRING, 0x80),
            ("ParticleBonesCheckDrawable", BOOL, 0xcd),
            ("ShadowForceDisable", BOOL, 0xce),
            ("HighDetailLODThreshold", FLOAT, 0x88),
            ("LowDetailLODThreshold", FLOAT, 0x8c),
            ("SwitchModelLODMode", BOOL, 0xcf),
            ("StaticModelLODMode", BOOL, 0xd0),
            ("ShowShadowWhileContained", BOOL, 0xd1),
            ("WadingParticleSys", 0x43f1_ce36, 0x90),
            ("AlphaCameraFadeOuterRadius", FLOAT, 0x94),
            ("AlphaCameraFadeInnerRadius", FLOAT, 0x98),
            ("AlphaCameraAtInnerRadius", FLOAT, 0x9c),
            ("BirthFadeTime", 0x23e6_50b8, 0xa0),
            ("MultiPlayerOnly", BOOL, 0xd2),
            ("AffectedByStealth", BOOL, 0xd3),
            ("InvertStealthOpacity", BOOL, 0xd4),
            ("HighDetailOnly", BOOL, 0xd5),
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

fn model_state(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    validate_attributes(node, &["ParseCondStateType", "ConditionsYes"], data.path)?;
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
    if let Some(value) = node.attribute("ConditionsYes") {
        write_model_conditions(value, start + 0xc, data)?;
    }
    let mut models = Vec::new();
    let mut material = None;
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "Model")) {
            models.push(child);
        } else if child.has_tag_name((SAGE_NS, "Material")) && material.is_none() {
            material = Some(child);
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
        array("Texture", 0x6e5d_40fb, start + 0x74),
        array("WeaponFireFXBone", 0xbd39_d1a1, start + 0x7c),
        array("WeaponRecoilBone", 0xbd39_d1a1, start + 0x84),
        array("WeaponMuzzleFlash", 0xbd39_d1a1, start + 0x8c),
        array("WeaponLaunchBone", 0xbd39_d1a1, start + 0x94),
        array("ParticleSysBone", 0x98c5_3fe6, start + 0x9c),
        array("FXEvent", 0x7599_1ad3, start + 0xa4),
        Entry::named("ShadowInfo", 0x82fe_7e88, Target::Null),
        array("Turret", 0xf309_5903, start + 0xb0),
        array("SubObject", 0x03a2_8c40, start + 0xb8),
    ]);
    Ok(Table::record(entries))
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
    for flag in value.split_ascii_whitespace() {
        let index = flags
            .iter()
            .take(0x3c * 8)
            .position(|name| name == flag)
            .ok_or_else(|| invalid(data.path, format!("invalid model condition {flag}")))?;
        data.payload[start + index / 8] |= 1 << (index % 8);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
