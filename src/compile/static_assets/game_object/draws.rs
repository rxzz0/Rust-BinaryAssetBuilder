use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, required_element,
    significant_children, validate_attributes,
};
use super::dynamics;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const SCRIPTED_MODEL_DRAW: u32 = 0x4ecf_2a4b;
const TRUCK_DRAW: u32 = 0xa44c_2999;
const SPOTLIGHT_DRAW: u32 = 0x42d7_103d;
const TANK_DRAW: u32 = 0x8a81_1d3e;
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

pub(in crate::compile) fn scripted_attributes() -> impl Iterator<Item = &'static str> {
    [
        "id",
        "ExtraPublicBone",
        "DependencySharedModelFlags",
        "BirthFadeTime",
        "MinLODRequired",
        "ProjectileBoneFeedbackEnabledSlots",
        "TrackMarks",
        "WadingParticleSys",
    ]
    .into_iter()
    .chain(SCRIPTED_FLAGS.iter().map(|&(name, _, _)| name))
    .chain(
        SCRIPTED_STRINGS
            .into_iter()
            .chain(SCRIPTED_REALS)
            .map(|(name, _)| name),
    )
}

fn scripted_flag(name: &'static str) -> (&'static str, u32, usize) {
    let &(_, offset, _) = SCRIPTED_FLAGS
        .iter()
        .find(|&&(field, _, _)| field == name)
        .expect("ScriptedModelDraw flag in layout");
    (name, BOOL, offset)
}

#[derive(Default)]
pub(super) struct DrawRelocations {
    extensions: Vec<(u32, u32, u32)>,
    bone_names: Vec<u32>,
}

impl DrawRelocations {
    pub(super) fn key(&self, pointer: u32) -> Option<(u32, u32)> {
        let index = self
            .extensions
            .partition_point(|&(start, _, _)| start <= pointer);
        let &(start, size, end) = self.extensions.get(index.checked_sub(1)?)?;
        let offset = pointer - start;
        if (0xd8..size).contains(&offset) {
            Some((start, offset))
        } else if (start + size..end).contains(&pointer) {
            let offset = if self.bone_names.binary_search(&pointer).is_ok() {
                offset - 0x50
            } else {
                offset
            };
            Some((end, offset))
        } else {
            None
        }
    }
}

pub(super) fn encode(
    group: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
    relocations: &mut DrawRelocations,
) -> io::Result<Target> {
    let modules: Vec<_> = significant_children(group).collect();
    let list = data.allocate(0x1cc, 8)?;
    let pointers = data.list(list, modules.len(), 4)?;
    let mut items = Vec::with_capacity(modules.len());
    for (index, node) in modules.into_iter().enumerate() {
        let pointer = pointers + index * 4;
        let (kind, module) = match node.tag_name().name() {
            name @ ("ScriptedModelDraw" | "TruckDraw" | "TankDraw" | "DynamicsDraw"
            | "SpotlightDraw")
                if node.has_tag_name((SAGE_NS, name)) =>
            {
                (
                    scripted_layout(name).0,
                    scripted_model(
                        node,
                        pointer,
                        data,
                        references,
                        reference_types,
                        material_float_names,
                        relocations,
                    )?,
                )
            }
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
            "StreamDraw" if node.has_tag_name((SAGE_NS, "StreamDraw")) => (
                super::stream_draw::KIND,
                super::stream_draw::encode(
                    node,
                    pointer,
                    data,
                    references,
                    reference_types,
                    material_float_names,
                )?,
            ),
            "PropDraw" if node.has_tag_name((SAGE_NS, "PropDraw")) => (
                0x148a_a464,
                prop_draw(node, pointer, data, references, reference_types)?,
            ),
            "TracerModelDraw" if node.has_tag_name((SAGE_NS, "TracerModelDraw")) => (
                0x5401_8e60,
                encode_tracer(node, pointer, data, references, reference_types)?,
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

fn prop_draw(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(node, &["id", "Model"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected PropDraw content"));
    }
    let start = data.allocate(pointer, 12)?;
    let parent = draw_base(node, 0x148a_a464, start, data)?;
    let model = super::passive_area::reference(
        node.attribute("Model").unwrap_or(""),
        "BaseRenderAssetType",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 8, model);
    Ok(Table::record(vec![
        parent,
        Entry::named("Model", 0xfcb1_06d5, Target::Data(start + 8)),
    ]))
}

fn draw_base(
    node: Node<'_, '_>,
    kind: u32,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Entry> {
    use super::behaviors_basic::{base, inherited};

    data.word(start, kind);
    data.word(start + 4, hash_name(required(node, "id", data.path)?));
    Ok(base(
        0x17f6_1728,
        inherited(
            0xd75d_93a2,
            Table::record(vec![
                Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
                Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
            ]),
        ),
    ))
}

fn encode_tracer(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    use super::behaviors_basic::weapon_slot;

    const REALS: [&str; 10] = [
        "MinLength",
        "MaxLength",
        "Width",
        "MinSpeed",
        "MaxSpeed",
        "SweepSpeed",
        "SpreadAngle",
        "MinTracersPerFrame",
        "MaxTracersPerFrame",
        "FrameLifeTime",
    ];
    const EXTRA: [(&str, u32, usize); 7] = [
        ("WeaponSlotType", 0x1848_c549, 0x30),
        ("WeaponSlotID", 0x23e6_50b8, 0x34),
        ("Texture", 0xbdb6_3f16, 0x38),
        ("UseAdditiveBlending", BOOL, 0x6c),
        ("TracerHitFx", 0x0d1c_8e83, 0x3c),
        ("TracerEmitFx", 0x0d1c_8e83, 0x40),
        ("TextureIndex", 0x23e6_50b8, 0x44),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(REALS)
        .chain(EXTRA.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x70)?;
    let mut entries = vec![draw_base(node, 0x5401_8e60, start, data)?];
    for (index, name) in REALS.into_iter().enumerate() {
        let default = if name == "SweepSpeed" { "1" } else { "0" };
        let field = start + 8 + 4 * index;
        data.word(
            field,
            finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
        entries.push(Entry::named(name, FLOAT, Target::Data(field)));
    }
    data.word(
        start + 0x30,
        weapon_slot(
            node.attribute("WeaponSlotType").unwrap_or("PRIMARY_WEAPON"),
            path,
        )?,
    );
    data.payload[start + 0x6c] = boolean(node, "UseAdditiveBlending", false, path)?;
    for (name, default, offset) in [("WeaponSlotID", "1", 0x34), ("TextureIndex", "0", 0x44)] {
        let word = node
            .attribute(name)
            .unwrap_or(default)
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, format!("invalid {name}")))?;
        data.word(start + offset, word as u32);
    }
    for (name, kind, offset) in [
        ("Texture", "Texture", 0x38),
        ("TracerHitFx", "FXList", 0x3c),
        ("TracerEmitFx", "FXList", 0x40),
    ] {
        let ordinal = super::passive_area::reference(
            node.attribute(name).unwrap_or(""),
            kind,
            path,
            references,
            reference_types,
        )?;
        data.word(start + offset, ordinal);
    }
    entries.extend(fields(start, &EXTRA));
    let mut children = significant_children(node).peekable();
    for (name, offset) in [("HeadColor", 0x48), ("TailColor", 0x58)] {
        let color = required_element(&mut children, name, path)?;
        let axes = ["r", "g", "b", "a"];
        validate_attributes(color, &axes, path)?;
        if has_content(color) {
            return Err(invalid(path, format!("unexpected {name} content")));
        }
        let mut channels = Vec::new();
        for (index, axis) in axes.into_iter().enumerate() {
            let field = start + offset + index * 4;
            data.word(
                field,
                finite_number(color.attribute(axis).unwrap_or("0"), axis, path)?.to_bits(),
            );
            channels.push(Entry::named(axis, FLOAT, Target::Data(field)));
        }
        entries.push(Entry::named(
            name,
            0x16ad_f350,
            Target::Table(Table::record(channels)),
        ));
    }
    let status =
        match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectStatusValidation"))) {
            Some(child) => Target::Table(super::secondary_damage::encode_status(
                child,
                start + 0x68,
                data,
            )?),
            None => Target::Null,
        };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected TracerModelDraw child"));
    }
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    Ok(Table::record(entries))
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

const TRUCK_FIELDS: [(&str, u32, usize, &str); 24] = [
    ("LeftFrontTireBone", STRING, 0xd8, ""),
    ("RightFrontTireBone", STRING, 0xe0, ""),
    ("LeftRearTireBone", STRING, 0xe8, ""),
    ("RightRearTireBone", STRING, 0xf0, ""),
    ("MidLeftFrontTireBone", STRING, 0xf8, ""),
    ("MidRightFrontTireBone", STRING, 0x100, ""),
    ("MidLeftRearTireBone", STRING, 0x108, ""),
    ("MidRightRearTireBone", STRING, 0x110, ""),
    ("MidLeftMidTireBone", STRING, 0x118, ""),
    ("MidRightMidTireBone", STRING, 0x120, ""),
    ("LeftFrontTireBone2", STRING, 0x128, ""),
    ("RightFrontTireBone2", STRING, 0x130, ""),
    ("LeftRearTireBone2", STRING, 0x138, ""),
    ("RightRearTireBone2", STRING, 0x140, ""),
    ("MidLeftMidTireBone2", STRING, 0x148, ""),
    ("MidRightMidTireBone2", STRING, 0x150, ""),
    ("TireRotationMultiplier", FLOAT, 0x158, "1"),
    ("TireRotationMultiplierFront", FLOAT, 0x15c, "1"),
    ("PowerslideRotationAddition", FLOAT, 0x160, "0"),
    ("CabBone", STRING, 0x164, ""),
    ("TrailerBone", STRING, 0x16c, ""),
    ("CabRotationMultiplier", FLOAT, 0x174, "1"),
    ("TrailerRotationMultiplier", FLOAT, 0x178, "1"),
    ("RotationDamping", FLOAT, 0x17c, "1"),
];

fn truck_fields(
    node: Node<'_, '_>,
    start: usize,
    particles: bool,
    maps: &[Node<'_, '_>],
    data: &mut BinaryData<'_>,
) -> io::Result<(Vec<Entry>, Option<usize>)> {
    const TREAD_FIELDS: [(&str, u32, usize, &str); 5] = [
        ("LeftTrack", STRING, 0, ""),
        ("RightTrack", STRING, 8, ""),
        ("SpeedMultiplier", FLOAT, 0x10, "1"),
        ("TreadPivotSpeedFraction", FLOAT, 0x14, "60%"),
        ("TreadDriveSpeedFraction", FLOAT, 0x18, "20%"),
    ];
    let path = data.path;
    let mut entries = Vec::with_capacity(TRUCK_FIELDS.len() + 2);
    for (name, token, offset, default) in TRUCK_FIELDS {
        let value = node.attribute(name);
        let offset = start + offset;
        if token == STRING {
            if let Some(value) = value {
                data.string(offset, value)?;
            }
        } else {
            data.word(
                offset,
                finite_number(value.unwrap_or(default), name, path)?.to_bits(),
            );
        }
        entries.push(Entry::named(name, token, Target::Data(offset)));
    }
    let particle_offset = particles
        .then(|| data.allocate(start + 0x180, 4))
        .transpose()?;
    let count = start + 0x184;
    let first = data.list(count, maps.len(), 0x1c)?;
    let mut items = Vec::with_capacity(maps.len());
    for (index, &map) in maps.iter().enumerate() {
        validate_attributes(map, &TREAD_FIELDS.map(|(name, _, _, _)| name), path)?;
        if has_content(map) {
            return Err(invalid(path, "unexpected TreadScrollingMap content"));
        }
        let start = first + 0x1c * index;
        let mut entries = Vec::with_capacity(TREAD_FIELDS.len());
        for (name, kind, offset, default) in TREAD_FIELDS {
            let field = start + offset;
            if kind == STRING {
                if let Some(value) = map.attribute(name) {
                    data.string(field, value)?;
                }
            } else {
                let value = map.attribute(name).unwrap_or(default);
                let number = if offset == 0x10 {
                    finite_number(value, name, path)?
                } else {
                    super::super::percentage(value, name, path)?
                };
                data.word(field, number.to_bits());
            }
            entries.push(Entry::named(name, kind, Target::Data(field)));
        }
        items.push((0, Target::Table(Table::record(entries))));
    }
    entries.push(Entry::named(
        "TreadScrollingMap",
        0xd1d3_e8e0,
        Target::Table(Table::array(count, items)),
    ));
    Ok((entries, particle_offset))
}

fn scripted_layout(name: &str) -> (u32, usize) {
    match name {
        "TruckDraw" => (TRUCK_DRAW, 0x18c),
        "TankDraw" => (TANK_DRAW, 0xe4),
        "SpotlightDraw" => (SPOTLIGHT_DRAW, 0xe0),
        "DynamicsDraw" => (dynamics::DRAW_KIND, dynamics::DRAW_SIZE),
        _ => (SCRIPTED_MODEL_DRAW, 0xd8),
    }
}

fn tank_fields(
    start: usize,
    particles: bool,
    bones: &[Node<'_, '_>],
    data: &mut BinaryData<'_>,
) -> io::Result<(Vec<Entry>, Option<usize>)> {
    const FIELDS: [&str; 2] = ["ArticulationBoneName", "ArticulationHelperBoneName"];
    let particle_offset = particles
        .then(|| data.allocate(start + 0xd8, 4))
        .transpose()?;
    let count = start + 0xdc;
    let first = data.list(count, bones.len(), 16)?;
    let mut items = Vec::with_capacity(bones.len());
    for (index, &bone) in bones.iter().enumerate() {
        validate_attributes(bone, &FIELDS, data.path)?;
        if has_content(bone) {
            return Err(invalid(data.path, "unexpected ArticulationBone content"));
        }
        let offset = first + index * 16;
        let mut entries = Vec::with_capacity(FIELDS.len());
        for (index, name) in FIELDS.into_iter().enumerate() {
            let field = offset + index * 8;
            if let Some(value) = bone.attribute(name) {
                data.string(field, value)?;
            }
            entries.push(Entry::named(name, 0x805d_25f4, Target::Data(field)));
        }
        items.push((0, Target::Table(Table::record(entries))));
    }
    Ok((
        vec![Entry::named(
            "ArticulationBone",
            0xc403_7fcb,
            Target::Table(Table::array(count, items)),
        )],
        particle_offset,
    ))
}

fn scripted_model(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    material_float_names: &mut Vec<u32>,
    relocations: &mut DrawRelocations,
) -> io::Result<Table> {
    let (kind, size) = scripted_layout(node.tag_name().name());
    let truck = kind == TRUCK_DRAW;
    let tank = kind == TANK_DRAW;
    let dynamics = kind == dynamics::DRAW_KIND;
    let spotlight = kind == SPOTLIGHT_DRAW;
    let spotlight_fields = ["RefreshTime", "SweepTime"];
    let allowed: Vec<_> = scripted_attributes()
        .chain(spotlight_fields.into_iter().filter(|_| spotlight))
        .chain(
            TRUCK_FIELDS
                .iter()
                .filter(|_| truck)
                .map(|&(name, _, _, _)| name),
        )
        .chain(
            dynamics
                .then_some(dynamics::DRAW_ATTRIBUTES)
                .into_iter()
                .flatten(),
        )
        .collect();
    validate_attributes(node, &allowed, data.path)?;
    let mut states = Vec::new();
    let mut animations = Vec::new();
    let mut attachments = Vec::new();
    let mut portals = Vec::new();
    let mut tread_particles = None;
    let mut tread_maps = Vec::new();
    let mut articulation_bones = Vec::new();
    let mut bone_volumes = None;
    let mut lifetime = None;
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "ModelConditionState")) {
            states.push(child);
        } else if child.has_tag_name((SAGE_NS, "AnimationState")) {
            animations.push(child);
        } else if child.has_tag_name((SAGE_NS, "AttachModel")) {
            attachments.push(child);
        } else if child.has_tag_name((SAGE_NS, "EmbedPortal")) {
            portals.push(child);
        } else if (truck || tank)
            && child.has_tag_name((SAGE_NS, "TreadTerrainParticles"))
            && tread_particles.is_none()
        {
            tread_particles = Some(child);
        } else if truck && child.has_tag_name((SAGE_NS, "TreadScrollingMap")) {
            tread_maps.push(child);
        } else if tank && child.has_tag_name((SAGE_NS, "ArticulationBone")) {
            articulation_bones.push(child);
        } else if dynamics && child.has_tag_name((SAGE_NS, "BoneVolumes")) && bone_volumes.is_none()
        {
            bone_volumes = Some(child);
        } else if dynamics && child.has_tag_name((SAGE_NS, "Lifetime")) && lifetime.is_none() {
            lifetime = Some(child);
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

    let start = data.allocate(pointer, size)?;
    data.word(start, kind);
    data.word(start + 4, hash_name(required(node, "id", data.path)?));
    let derived = match kind {
        TRUCK_DRAW => Some(truck_fields(
            node,
            start,
            tread_particles.is_some(),
            &tread_maps,
            data,
        )?),
        TANK_DRAW => Some(tank_fields(
            start,
            tread_particles.is_some(),
            &articulation_bones,
            data,
        )?),
        dynamics::DRAW_KIND => Some((
            dynamics::draw_fields(
                node,
                start,
                bone_volumes,
                lifetime,
                data,
                &mut relocations.bone_names,
            )?,
            None,
        )),
        _ => None,
    };
    if derived.is_some() {
        relocations
            .extensions
            .push((start as u32, size as u32, data.payload.len() as u32));
    }
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

    if let Some(value) = node.attribute("MinLODRequired") {
        let lod =
            super::super::global_ui::enum_value(value, &["LOW", "MEDIUM", "HIGH"], data.path)?;
        data.word(start + 0x20, lod);
    }
    if let Some(value) = node.attribute("ProjectileBoneFeedbackEnabledSlots") {
        let mut flags = 0;
        for slot in value.split_ascii_whitespace() {
            flags |= 1 << super::behaviors_basic::weapon_slot(slot, data.path)?;
        }
        data.word(start + 0x24, flags);
    }
    for (name, kind, offset) in [
        ("TrackMarks", "Texture", 0x28),
        ("WadingParticleSys", "FXParticleSystemTemplate", 0x90),
    ] {
        if let Some(value) = node.attribute(name) {
            let ordinal = super::passive_area::reference(
                value,
                kind,
                data.path,
                references,
                reference_types,
            )?;
            data.word(start + offset, ordinal);
        }
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
    let attachments = attach_models(attachments, start + 0xb4, data)?;
    let portals = embed_portals(portals, start + 0xbc, data)?;

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
        Entry::named("AttachModel", 0xb4e5_23bf, Target::Table(attachments)),
        Entry::named("EmbedPortal", 0xafc5_e3e3, Target::Table(portals)),
    ]);
    let table = Table::record(entries);
    if spotlight {
        let mut fields = vec![super::behaviors_basic::base(SCRIPTED_MODEL_DRAW, table)];
        for (index, name) in spotlight_fields.into_iter().enumerate() {
            let offset = start + 0xd8 + index * 4;
            let value =
                super::super::sage_time(node.attribute(name).unwrap_or("1s"), name, data.path)?;
            data.word(offset, value.to_bits());
            fields.push(Entry::named(name, 0x9986_69e8, Target::Data(offset)));
        }
        Ok(Table::record(fields))
    } else if let Some((mut entries, particle_offset)) = derived {
        if !dynamics {
            let particles = if let Some((particles, offset)) = tread_particles.zip(particle_offset)
            {
                let value = super::super::text_value(particles, data.path)?;
                let ordinal = super::passive_area::reference(
                    value,
                    "TerrainParticleMapping",
                    data.path,
                    references,
                    reference_types,
                )?;
                data.word(offset, ordinal);
                Target::Data(offset)
            } else {
                Target::Null
            };
            entries.insert(
                if truck { TRUCK_FIELDS.len() } else { 0 },
                Entry::named("TreadTerrainParticles", 0xa6ff_4551, particles),
            );
        }
        entries.insert(0, super::behaviors_basic::base(SCRIPTED_MODEL_DRAW, table));
        Ok(Table::record(entries))
    } else {
        Ok(table)
    }
}

const WEAPON_BONE_LISTS: [(&str, usize); 4] = [
    ("WeaponFireFXBone", 0x7c),
    ("WeaponRecoilBone", 0x84),
    ("WeaponMuzzleFlash", 0x8c),
    ("WeaponLaunchBone", 0x94),
];

fn attach_models(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    const ITEM_SIZE: usize = 0x50;
    let start = data.list(count, nodes.len(), ITEM_SIZE)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &["FlagMask", "Bone"], data.path)?;
        let mut children = significant_children(node);
        let model = required_element(&mut children, "Model", data.path)?;
        if children.next().is_some() || has_content(model) {
            return Err(invalid(data.path, "unexpected AttachModel content"));
        }
        validate_attributes(model, &["Object", "Probability"], data.path)?;

        let offset = start + index * ITEM_SIZE;
        if let Some(value) = node.attribute("FlagMask") {
            write_model_conditions(value, offset, data)?;
        }
        if let Some(value) = node.attribute("Bone") {
            data.string(offset + 0x3c, value)?;
        }
        if let Some(value) = model.attribute("Object") {
            data.string(offset + 0x44, value)?;
        }
        let probability = super::super::sage_int(
            model.attribute("Probability").unwrap_or("-1"),
            "Probability",
            data.path,
        )?;
        data.word(offset + 0x4c, probability);

        let model_fields = Table::record(
            fields(
                offset,
                &[
                    ("Object", 0x805d_25f4, 0x44),
                    ("Probability", 0x23e6_50b8, 0x4c),
                ],
            )
            .collect(),
        );
        let mut entries: Vec<_> = fields(
            offset,
            &[("FlagMask", 0x67db_bfc9, 0), ("Bone", STRING, 0x3c)],
        )
        .collect();
        entries.push(Entry::named(
            "Model",
            0x85e3_17bd,
            Target::Table(model_fields),
        ));
        items.push((0, Target::Table(Table::record(entries))));
    }
    Ok(Table::array(count, items))
}

fn embed_portals(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    const PORTAL_TYPES: &[&str] = &[
        "NORMAL",
        "PORTAL",
        "WALK_PORTAL",
        "CLIMB_PORTAL",
        "PRE_CLIMB_PORTAL",
        "BEACON",
        "SPLINE_CATMULL_ROM",
        "FAKE_PATHFIND_PORTAL",
        "MINESHAFT_PORTAL",
    ];
    const ITEM_SIZE: usize = 0x0c;
    let start = data.list(count, nodes.len(), ITEM_SIZE)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &["PortalType", "BonePrefix"], data.path)?;
        if has_content(node) {
            return Err(invalid(data.path, "unexpected EmbedPortal content"));
        }

        let offset = start + index * ITEM_SIZE;
        if let Some(value) = node.attribute("PortalType") {
            let portal_type = PORTAL_TYPES
                .iter()
                .position(|&variant| variant == value.trim())
                .ok_or_else(|| invalid(data.path, format!("invalid PortalType {value:?}")))?;
            data.word(offset, portal_type as u32);
        }
        if let Some(value) = node.attribute("BonePrefix") {
            data.string(offset + 4, value)?;
        }
        let entries = fields(
            offset,
            &[("PortalType", 0x7459_48b5, 0), ("BonePrefix", STRING, 4)],
        );
        items.push((0, Target::Table(Table::record(entries.collect()))));
    }
    Ok(Table::array(count, items))
}

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
        &[
            "ParseCondStateType",
            "Name",
            "ConditionsYes",
            "Skeleton",
            "PortraitImage",
            "ButtonImage",
            "OverrideTooltip",
            "RetainSubObjects",
            "id",
        ],
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
    for (name, offset) in [
        ("Name", 0x04),
        ("Skeleton", 0x48),
        ("OverrideTooltip", 0x58),
        ("id", 0x60),
    ] {
        if let Some(value) = node.attribute(name) {
            data.string(start + offset, value)?;
        }
    }
    for (name, offset) in [("PortraitImage", 0x50), ("ButtonImage", 0x54)] {
        if let Some(value) = node.attribute(name).filter(|value| !value.is_empty()) {
            let reference = asset_reference(value, "PackedTextureImage", data.path)?;
            references.push(reference.into());
            reference_types.push(reference.type_id);
            data.word(start + offset, references.len() as u32);
        }
    }
    let mut weapon_bones: [Vec<_>; 4] = std::array::from_fn(|_| Vec::new());
    let mut models = Vec::new();
    let mut material = None;
    let mut textures = Vec::new();
    let mut particles = Vec::new();
    let mut fx_events = Vec::new();
    let mut sub_objects = Vec::new();
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
        } else if child.has_tag_name((SAGE_NS, "FXEvent")) {
            fx_events.push(child);
        } else if child.has_tag_name((SAGE_NS, "SubObject")) {
            sub_objects.push(child);
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
    let fx = super::animation::encode_fx_events(
        fx_events,
        start + 0xa4,
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
        Entry::named("FXEvent", 0x7599_1ad3, Target::Table(fx)),
        Entry::named("ShadowInfo", 0x82fe_7e88, shadow),
        Entry::named(
            "Turret",
            0xf309_5903,
            Target::Table(encode_turrets(turrets, start + 0xb0, data)?),
        ),
        Entry::named(
            "SubObject",
            0x03a2_8c40,
            Target::Table(encode_state_sub_objects(sub_objects, start + 0xb8, data)?),
        ),
    ]);
    Ok(Table::record(entries))
}

fn encode_state_sub_objects(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let start = data.list(count, nodes.len(), 4)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &[], data.path)?;
        if node.children().any(|child| child.is_element()) {
            return Err(invalid(
                data.path,
                "SubObject cannot contain child elements",
            ));
        }
        let value: String = node
            .children()
            .filter_map(|child| child.is_text().then(|| child.text()).flatten())
            .collect();
        if value.is_empty() {
            return Err(invalid(data.path, "SubObject is missing its identifier"));
        }
        let offset = start + index * 4;
        data.word(offset, hash_name(&value));
        items.push((0, Target::Data(offset)));
    }
    Ok(Table::array(count, items))
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
        validate_attributes(node, &["WeaponSlotID", "WeaponSlotType", "BoneName"], path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected weapon bone content"));
        }
        let offset = start + index * 0x10;
        let slot = super::super::global_ui::enum_value(
            node.attribute("WeaponSlotType").unwrap_or("PRIMARY_WEAPON"),
            SLOTS,
            path,
        )?;
        let id = node
            .attribute("WeaponSlotID")
            .unwrap_or("1")
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, "invalid WeaponSlotID"))?;
        data.word(offset, id as u32);
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

pub(super) fn boolean(
    node: Node<'_, '_>,
    name: &str,
    default: bool,
    path: &std::path::Path,
) -> io::Result<u8> {
    match node.attribute(name) {
        None => Ok(u8::from(default)),
        Some("true" | "True" | "1") => Ok(1),
        Some("false" | "False" | "0") => Ok(0),
        Some(value) => Err(invalid(path, format!("invalid {name} {value}"))),
    }
}

pub(in crate::compile::static_assets) fn write_model_conditions(
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

pub(in crate::compile::static_assets) fn model_condition_index(
    flag: &str,
    path: &std::path::Path,
) -> io::Result<usize> {
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
