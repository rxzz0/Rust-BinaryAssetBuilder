use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, sage_time, significant_children,
    validate_attributes,
};
use super::draws::{boolean, fields, write_model_conditions};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

const REAL: u32 = 0x6458_64f3;
const STRING: u32 = 0x1a29_33aa;
const BOOL: u32 = 0x77e3_f4f7;
const INT: u32 = 0x23e6_50b8;

pub(super) fn encode(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let start = data.list(count, nodes.len(), 0x8c)?;
    let mut items = Vec::with_capacity(nodes.len());
    let mut encoder = AnimationEncoder {
        data,
        references,
        reference_types,
    };
    for (index, node) in nodes.into_iter().enumerate() {
        items.push((0, Target::Table(encoder.state(node, start + index * 0x8c)?)));
    }
    Ok(Table::array(count, items))
}

pub(super) fn encode_particles(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    AnimationEncoder {
        data,
        references,
        reference_types,
    }
    .particles(nodes, count)
}

pub(super) fn encode_fx_events(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    AnimationEncoder {
        data,
        references,
        reference_types,
    }
    .fx_events(nodes, count)
}

struct AnimationEncoder<'a, 'path> {
    data: &'a mut BinaryData<'path>,
    references: &'a mut Vec<AssetReference>,
    reference_types: &'a mut Vec<u32>,
}

impl AnimationEncoder<'_, '_> {
    fn state(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        const FIELDS: &[(&str, u32, usize)] = &[
            ("ParseCondStateType", 0x1323_b870, 0),
            ("AnimNickName", STRING, 4),
            ("ConditionsYes", 0x67db_bfc9, 0xc),
            ("Name", STRING, 0x48),
            ("StateName", STRING, 0x50),
            ("Flags", 0x5ec2_b352, 0x58),
            ("ShareAnimation", BOOL, 0x88),
            ("EnteringStateFX", 0x0d1c_8e83, 0x5c),
            ("FrameForPristineBonePositions", INT, 0x60),
            ("AllowRepeatInRandomPick", BOOL, 0x89),
            ("SimilarRestart", BOOL, 0x8a),
        ];
        let path = self.data.path;
        check_attributes(node, FIELDS, path)?;
        let parse = required(node, "ParseCondStateType", path)?;
        self.data.word(
            start,
            enum_value(
                parse,
                &["PARSE_NORMAL", "PARSE_DEFAULT", "PARSE_TRANSITION"],
                "ParseCondStateType",
                path,
            )?,
        );
        for (name, offset) in [("AnimNickName", 4), ("Name", 0x48), ("StateName", 0x50)] {
            self.string(node, name, start + offset)?;
        }
        write_model_conditions(
            node.attribute("ConditionsYes").unwrap_or(""),
            start + 0xc,
            self.data,
        )?;
        let mut flags = 0;
        for flag in node
            .attribute("Flags")
            .unwrap_or("NONE")
            .split_ascii_whitespace()
        {
            flags |= 1
                << enum_value(
                    flag,
                    &[
                        "NONE",
                        "RANDOMSTART",
                        "START_FRAME_FIRST",
                        "START_FRAME_LAST",
                        "RESTART_ANIM_WHEN_COMPLETE",
                        "MAINTAIN_FRAME_ACROSS_STATES",
                        "MAINTAIN_FRAME_ACROSS_STATES2",
                        "MAINTAIN_FRAME_ACROSS_STATES3",
                        "MAINTAIN_FRAME_ACROSS_STATES4",
                        "DO_NOT_PLAY_WHEN_UNPOWERED",
                        "ADJUST_HEIGHT_BY_CONSTRUCTION_PERCENT",
                        "IGNORE_MOVEMENT_SPEED",
                    ],
                    "animation state flag",
                    path,
                )?;
        }
        self.data.word(start + 0x58, flags);
        self.reference(node, "EnteringStateFX", "FXList", start + 0x5c)?;
        self.integer(node, "FrameForPristineBonePositions", "0", start + 0x60)?;
        for (name, offset) in [
            ("ShareAnimation", 0x88),
            ("AllowRepeatInRandomPick", 0x89),
            ("SimilarRestart", 0x8a),
        ] {
            self.data.payload[start + offset] = boolean(node, name, false, path)?;
        }

        let mut animations = Vec::new();
        let mut particles = Vec::new();
        let mut fx_events = Vec::new();
        let mut lua_events = Vec::new();
        let mut script = None;
        for child in significant_children(node) {
            if child.has_tag_name((SAGE_NS, "Animation")) {
                animations.push(child);
            } else if child.has_tag_name((SAGE_NS, "ParticleSysBone")) {
                particles.push(child);
            } else if child.has_tag_name((SAGE_NS, "FXEvent")) {
                fx_events.push(child);
            } else if child.has_tag_name((SAGE_NS, "LuaEvent")) {
                lua_events.push(child);
            } else if child.has_tag_name((SAGE_NS, "Script")) && script.is_none() {
                script = Some(child);
            } else {
                return Err(invalid(
                    path,
                    format!(
                        "unsupported or repeated AnimationState child {}",
                        child.tag_name().name()
                    ),
                ));
            }
        }
        let first = self.data.list(start + 0x64, animations.len(), 0x3c)?;
        let mut items = Vec::with_capacity(animations.len());
        for (index, animation) in animations.into_iter().enumerate() {
            items.push((
                0,
                Target::Table(self.animation(animation, first + index * 0x3c)?),
            ));
        }
        let script = if let Some(script) = script {
            validate_attributes(script, &[], path)?;
            if script.children().any(|child| child.is_element()) {
                return Err(invalid(path, "Script cannot contain child elements"));
            }
            let text: String = script
                .children()
                .filter(Node::is_text)
                .filter_map(|child| child.text())
                .collect();
            let offset = self.data.allocate(start + 0x6c, 8)?;
            self.data.string(offset, &text)?;
            Target::Data(offset)
        } else {
            Target::Null
        };
        let fx = self.fx_events(fx_events, start + 0x70)?;
        let lua = self.lua_events(lua_events, start + 0x78)?;
        let particle_table = self.particles(particles, start + 0x80)?;
        let mut entries: Vec<_> = fields(start, FIELDS).collect();
        entries.extend([
            Entry::named(
                "Animation",
                0x58a1_75d9,
                Target::Table(Table::array(start + 0x64, items)),
            ),
            Entry::named("Script", 0x1847_c490, script),
            Entry::named("FXEvent", 0x7599_1ad3, Target::Table(fx)),
            Entry::named("LuaEvent", 0xda05_eb72, Target::Table(lua)),
            Entry::named(
                "ParticleSysBone",
                0x98c5_3fe6,
                Target::Table(particle_table),
            ),
        ]);
        Ok(Table::record(entries))
    }

    fn fx_events(&mut self, events: Vec<Node<'_, '_>>, count: usize) -> io::Result<Table> {
        const FIELDS: &[(&str, u32, usize)] = &[
            ("Frame", INT, 0),
            ("FrameStep", INT, 4),
            ("FrameStop", INT, 8),
            ("FireWhenSkipped", BOOL, 0x18),
            ("Effect", 0x0d1c_8e83, 0xc),
            ("Bone", STRING, 0x10),
        ];
        let path = self.data.path;
        let first = self.data.list(count, events.len(), 0x1c)?;
        let mut items = Vec::with_capacity(events.len());
        for (index, node) in events.into_iter().enumerate() {
            check_attributes(node, FIELDS, path)?;
            if has_content(node) {
                return Err(invalid(path, "FXEvent cannot contain content"));
            }
            let start = first + index * 0x1c;
            for &(name, _, offset) in &FIELDS[..3] {
                self.integer(node, name, "0", start + offset)?;
            }
            self.data.payload[start + 0x18] = boolean(node, "FireWhenSkipped", false, path)?;
            self.reference(node, "Effect", "FXList", start + 0xc)?;
            if let Some(bone) = node.attribute("Bone") {
                self.data.string(start + 0x10, bone)?;
            }
            items.push((
                0,
                Target::Table(Table::record(fields(start, FIELDS).collect())),
            ));
        }
        Ok(Table::array(count, items))
    }

    fn lua_events(&mut self, events: Vec<Node<'_, '_>>, count: usize) -> io::Result<Table> {
        const FIELDS: &[(&str, u32, usize)] = &[
            ("Frame", INT, 0),
            ("Data", STRING, 4),
            ("Trigger", 0xc155_8278, 12),
        ];
        let path = self.data.path;
        let first = self.data.list(count, events.len(), 16)?;
        let mut items = Vec::with_capacity(events.len());
        for (index, node) in events.into_iter().enumerate() {
            check_attributes(node, FIELDS, path)?;
            if has_content(node) {
                return Err(invalid(path, "LuaEvent cannot contain content"));
            }
            let start = first + index * 16;
            self.integer(node, "Frame", "0", start)?;
            self.string(node, "Data", start + 4)?;
            let trigger = enum_value(
                node.attribute("Trigger").unwrap_or("TRIGGER_ONFRAME"),
                &[
                    "TRIGGER_ONFRAME",
                    "TRIGGER_ONSTATEENTER",
                    "TRIGGER_ONSTATELEAVE",
                ],
                "LuaEvent Trigger",
                path,
            )?;
            self.data.word(start + 12, trigger);
            items.push((
                0,
                Target::Table(Table::record(fields(start, FIELDS).collect())),
            ));
        }
        Ok(Table::array(count, items))
    }

    fn animation(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        const FIELDS: &[(&str, u32, usize)] = &[
            ("Flags", STRING, 0),
            ("AnimationName", 0xfe2d_cb83, 8),
            ("AnimationMode", 0xe3cd_c786, 0xc),
            ("AnimNickName", STRING, 0x10),
            ("Distance", REAL, 0x18),
            ("AnimationBlendTime", REAL, 0x1c),
            ("AnimationMustCompleteBlend", BOOL, 0x38),
            ("AnimationSpeedFactorMin", REAL, 0x20),
            ("AnimationSpeedFactorMax", REAL, 0x24),
            ("AnimationAbsoluteTime", 0x9986_69e8, 0x28),
            ("UseWeaponTiming", BOOL, 0x39),
            ("WeaponTimingOrdering", 0x1848_c549, 0x2c),
            ("WeaponTimingSlotID", INT, 0x30),
            ("AnimationPriority", INT, 0x34),
        ];
        let path = self.data.path;
        check_attributes(node, FIELDS, path)?;
        if has_content(node) {
            return Err(invalid(path, "Animation cannot contain content"));
        }
        self.string(node, "Flags", start)?;
        self.reference(node, "AnimationName", "W3DAnimation", start + 8)?;
        self.data.word(
            start + 0xc,
            enum_value(
                node.attribute("AnimationMode").unwrap_or("MANUAL"),
                &[
                    "MANUAL",
                    "LOOP",
                    "ONCE",
                    "LOOP_BACKWARDS",
                    "ONCE_BACKWARDS",
                    "LOOP_PINGPONG",
                    "PLAY_TO_FRAME",
                    "MATCH_UNPACKING",
                ],
                "AnimationMode",
                path,
            )?,
        );
        self.string(node, "AnimNickName", start + 0x10)?;
        for (name, default, offset) in [
            ("Distance", "0", 0x18),
            ("AnimationBlendTime", "5", 0x1c),
            ("AnimationSpeedFactorMin", "1", 0x20),
            ("AnimationSpeedFactorMax", "1", 0x24),
        ] {
            self.data.word(
                start + offset,
                finite_number(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
            );
        }
        self.data.word(
            start + 0x28,
            sage_time(
                node.attribute("AnimationAbsoluteTime").unwrap_or("0s"),
                "AnimationAbsoluteTime",
                path,
            )?
            .to_bits(),
        );
        self.data.word(
            start + 0x2c,
            enum_value(
                node.attribute("WeaponTimingOrdering")
                    .unwrap_or("PRIMARY_WEAPON"),
                &[
                    "PRIMARY_WEAPON",
                    "SECONDARY_WEAPON",
                    "TERTIARY_WEAPON",
                    "QUATERNARY_WEAPON",
                    "QUINARY_WEAPON",
                    "NO_WEAPON",
                ],
                "WeaponTimingOrdering",
                path,
            )?,
        );
        self.integer(node, "WeaponTimingSlotID", "1", start + 0x30)?;
        self.integer(node, "AnimationPriority", "1", start + 0x34)?;
        self.data.payload[start + 0x38] = boolean(node, "AnimationMustCompleteBlend", false, path)?;
        self.data.payload[start + 0x39] = boolean(node, "UseWeaponTiming", false, path)?;
        Ok(Table::record(fields(start, FIELDS).collect()))
    }

    fn particles(&mut self, nodes: Vec<Node<'_, '_>>, count: usize) -> io::Result<Table> {
        const FIELDS: &[(&str, u32, usize)] = &[
            ("BoneName", STRING, 0),
            ("FXParticleSystemTemplate", 0x43f1_ce36, 8),
            ("FollowBone", BOOL, 0x20),
            ("FXTrigger", 0x808b_85be, 0xc),
            ("FXAction", 0x7c49_bc33, 0x10),
            ("PersistID", INT, 0x14),
            ("OnlyIfOnWater", BOOL, 0x21),
            ("OnlyIfOnLand", BOOL, 0x22),
            ("id", 0x805d_25f4, 0x18),
        ];
        let path = self.data.path;
        let start = self.data.list(count, nodes.len(), 0x24)?;
        let mut items = Vec::with_capacity(nodes.len());
        for (index, node) in nodes.into_iter().enumerate() {
            check_attributes(node, FIELDS, path)?;
            if has_content(node) {
                return Err(invalid(path, "ParticleSysBone cannot contain content"));
            }
            let offset = start + index * 0x24;
            self.string(node, "BoneName", offset)?;
            self.reference(
                node,
                "FXParticleSystemTemplate",
                "FXParticleSystemTemplate",
                offset + 8,
            )?;
            for (name, variants, field) in [
                (
                    "FXTrigger",
                    &["NONE", "CATAPULT_ROCK", "TREBUCHET_ROCK"][..],
                    0xc,
                ),
                ("FXAction", &["NONE", "HOLD", "KILL", "SPAWN"][..], 0x10),
            ] {
                self.data.word(
                    offset + field,
                    enum_value(node.attribute(name).unwrap_or("NONE"), variants, name, path)?,
                );
            }
            self.integer(node, "PersistID", "0", offset + 0x14)?;
            self.string(node, "id", offset + 0x18)?;
            for (name, field) in [
                ("FollowBone", 0x20),
                ("OnlyIfOnWater", 0x21),
                ("OnlyIfOnLand", 0x22),
            ] {
                self.data.payload[offset + field] = boolean(node, name, false, path)?;
            }
            items.push((
                0,
                Target::Table(Table::record(fields(offset, FIELDS).collect())),
            ));
        }
        Ok(Table::array(count, items))
    }

    fn reference(
        &mut self,
        node: Node<'_, '_>,
        name: &str,
        kind: &str,
        offset: usize,
    ) -> io::Result<()> {
        if let Some(value) = node
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
        {
            let name = value.split_once('\\').map_or(value, |(name, _)| name);
            let id = asset_reference(name, kind, self.data.path)?;
            self.references.push(id.into());
            self.reference_types.push(id.type_id);
            self.data.word(offset, self.references.len() as u32);
        }
        Ok(())
    }

    fn string(&mut self, node: Node<'_, '_>, name: &str, offset: usize) -> io::Result<()> {
        if let Some(text) = node.attribute(name).filter(|text| !text.is_empty()) {
            self.data.string(offset, text)?;
        }
        Ok(())
    }

    fn integer(
        &mut self,
        node: Node<'_, '_>,
        name: &str,
        default: &str,
        offset: usize,
    ) -> io::Result<()> {
        let value = node
            .attribute(name)
            .unwrap_or(default)
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(self.data.path, format!("invalid {name}")))?;
        self.data.word(offset, value as u32);
        Ok(())
    }
}

fn check_attributes(
    node: Node<'_, '_>,
    fields: &[(&str, u32, usize)],
    path: &Path,
) -> io::Result<()> {
    let names: Vec<_> = fields.iter().map(|&(name, _, _)| name).collect();
    validate_attributes(node, &names, path)
}

fn enum_value(value: &str, values: &[&str], name: &str, path: &Path) -> io::Result<u32> {
    values
        .iter()
        .position(|&candidate| candidate == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("invalid {name} {value}")))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn draw_animation_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-animation.xml"
        ))
        .unwrap();
        let node = doc.root_element().first_element_child().unwrap();
        let actual = compile_asset(node, Path::new("game-object-animation.xml"), "").unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-animation.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-animation.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-animation.manifest"
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
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.assets[0].references.iter())
        );
    }
}
