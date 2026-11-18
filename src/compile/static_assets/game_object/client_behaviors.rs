use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, fast_hash, hash_name, sage_int, significant_children,
    validate_attributes,
};
use super::draws::write_model_conditions;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const ANIMATION_SOUND: u32 = 0x27ef_791d;
const LOOP: u32 = 0x5a3e_dbf4;
const SELECTOR: u32 = 0x327c_fb76;
const UPGRADE_SELECTOR: u32 = 0xf911_5aa9;
const SOUND: u32 = 0x63d7_b468;
const INT: u32 = 0x23e6_50b8;

pub(super) fn encode(
    group: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    let mut encoder = AudioEncoder {
        data,
        references,
        reference_types,
    };
    let modules: Vec<_> = significant_children(group).collect();
    let count = encoder.data.allocate(0x1e4, 8)?;
    let pointers = encoder.data.list(count, modules.len(), 4)?;
    let mut items = Vec::with_capacity(modules.len());
    for (index, node) in modules.into_iter().enumerate() {
        let (kind, name, tag, size) = match node.tag_name().name() {
            "AnimationSoundClientBehavior" => (ANIMATION_SOUND, "Sound", 0xeb6c_6024, 0x18),
            "ModelConditionAudioLoopClientBehavior" => {
                (LOOP, "ModelConditionSound", 0xd832_6ddd, 0x7c)
            }
            "ModelConditionSoundSelectorClientBehavior" => {
                (SELECTOR, "Override", 0xbfa5_beba, 0x84)
            }
            "UpgradeSoundSelectorClientBehavior" => {
                (UPGRADE_SELECTOR, "Override", 0x94f2_5492, 0x94)
            }
            name => {
                return Err(invalid(
                    encoder.data.path,
                    format!("unsupported client behavior {name}"),
                ));
            }
        };
        if node.tag_name().namespace() != Some(SAGE_NS) {
            return Err(invalid(
                encoder.data.path,
                "unexpected client behavior namespace",
            ));
        }
        let allowed: &[_] = if kind == ANIMATION_SOUND {
            &["id", "MaxUpdateRangeCap"]
        } else {
            &["id"]
        };
        validate_attributes(node, allowed, encoder.data.path)?;
        let children = elements(node, name, encoder.data.path)?;
        if matches!(kind, ANIMATION_SOUND | SELECTOR) && children.is_empty() {
            return Err(invalid(
                encoder.data.path,
                format!("client behavior requires {name}"),
            ));
        }
        let is_animation = kind == ANIMATION_SOUND;
        let start = encoder
            .data
            .allocate(pointers + index * 4, if is_animation { 20 } else { 16 })?;
        encoder.data.word(start, kind);
        encoder.data.word(
            start + 4,
            hash_name(required(node, "id", encoder.data.path)?),
        );
        let mut range = Target::Null;
        let count = start + if is_animation { 12 } else { 8 };
        if is_animation && let Some(value) = node.attribute("MaxUpdateRangeCap") {
            let value = super::super::finite_number(value, "MaxUpdateRangeCap", encoder.data.path)?;
            if value <= 0.0 {
                return Err(invalid(
                    encoder.data.path,
                    "MaxUpdateRangeCap must be positive",
                ));
            }
            let offset = encoder.data.allocate(start + 8, 4)?;
            encoder.data.word(offset, value.to_bits());
            range = Target::Data(offset);
        }
        let first = encoder.data.list(count, children.len(), size)?;
        let mut entries = Vec::with_capacity(children.len());
        for (index, child) in children.into_iter().enumerate() {
            let offset = first + index * size;
            entries.push((
                0,
                Target::Table(if kind == LOOP {
                    encoder.sound_loop(child, offset)?
                } else if is_animation {
                    encoder.animation_sound(child, offset)?
                } else {
                    encoder.sound_override(child, offset, kind == UPGRADE_SELECTOR)?
                }),
            ));
        }
        let module = Table::record(vec![
            Entry::new(0x19b0_42a0, INT, Target::Data(start)),
            Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
        ]);
        let mut fields = vec![base(
            0x7b8c_5772,
            Table::record(vec![base(0xd75d_93a2, module)]),
        )];
        if is_animation {
            fields.push(Entry::named("MaxUpdateRangeCap", 0xbc2f_1cbf, range));
        }
        fields.push(list(name, tag, count, entries));
        let fields = Table::record(fields);
        items.push((
            0,
            Target::Table(Table::sequence(vec![Entry::new(
                2,
                kind,
                Target::Table(fields),
            )])),
        ));
    }
    Ok(Target::Table(Table::array(count, items)))
}

struct AudioEncoder<'a, 'path> {
    data: &'a mut BinaryData<'path>,
    references: &'a mut Vec<AssetReference>,
    reference_types: &'a mut Vec<u32>,
}

pub(super) fn encode_object_audio(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    let mut encoder = AudioEncoder {
        data,
        references,
        reference_types,
    };
    match node.tag_name().name() {
        "AudioArrayVoice" => encoder.audio_array(Some(node), 0x208, true),
        "AudioArraySound" => encoder.audio_array(Some(node), 0x20c, false),
        "EvaEventArray" => encoder.eva_array(node),
        name => Err(invalid(
            encoder.data.path,
            format!("unsupported GameObject audio array {name}"),
        )),
    }
}

impl AudioEncoder<'_, '_> {
    fn animation_sound(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        let path = self.data.path;
        validate_attributes(
            node,
            &["Sound", "RequiredMC", "ExcludedMC", "Animation", "Frame"],
            path,
        )?;
        require_empty(node, path)?;
        required(node, "Sound", path)?;
        required(node, "Animation", path)?;
        for attribute in node.attributes() {
            let value = attribute.value();
            match attribute.name() {
                "Sound" => {
                    let sound = self.reference(value, "BaseAudioEventInfo")?;
                    self.data.word(start, sound);
                }
                "Animation" => {
                    let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                    let animation = asset_reference(plain, "W3DAnimation", path)?;
                    self.reference_types.push(animation.type_id);
                    self.data.word(start + 12, animation.instance_id);
                }
                _ => {}
            }
        }
        let mut entries = vec![field("Sound", SOUND, start)];
        for (name, offset) in [("RequiredMC", 4), ("ExcludedMC", 8)] {
            let target = if let Some(value) = node.attribute(name) {
                let first = self.data.allocate(start + offset, 0x3c)?;
                write_model_conditions(value, first, self.data)?;
                Target::Data(first)
            } else {
                Target::Null
            };
            entries.push(Entry::named(name, 0x67db_bfc9, target));
        }
        entries.push(field("Animation", 0x2648_0425, start + 12));
        let frames = required(node, "Frame", path)?.split_ascii_whitespace();
        let count = start + 16;
        let length = frames.clone().count();
        let first = self.data.list(count, length, 4)?;
        let mut items = Vec::with_capacity(length);
        for (index, frame) in frames.enumerate() {
            let offset = first + 4 * index;
            self.data
                .word(offset, sage_int(frame, "animation sound frame", path)?);
            items.push((0, Target::Data(offset)));
        }
        entries.push(list("Frame", 0x49ef_147d, count, items));
        Ok(Table::record(entries))
    }

    fn eva_array(&mut self, node: Node<'_, '_>) -> io::Result<Target> {
        validate_attributes(node, &[], self.data.path)?;
        let children = elements(node, "EvaEntry", self.data.path)?;
        let start = self.data.allocate(0x210, 8)?;
        let first = self.data.list(start, children.len(), 8)?;
        let mut items = Vec::with_capacity(children.len());
        for (index, child) in children.into_iter().enumerate() {
            validate_attributes(child, &["EvaEvent", "EvaType"], self.data.path)?;
            require_empty(child, self.data.path)?;
            let offset = first + index * 8;
            let event = self.reference(required(child, "EvaEvent", self.data.path)?, "EvaEvent")?;
            self.data.word(offset, event);
            self.data.word(
                offset + 4,
                audio_type(child, AudioType::Eva, self.data.path)?,
            );
            items.push((
                0,
                Target::Table(Table::record(vec![
                    field("EvaEvent", 0xc4ec_43b1, offset),
                    field("EvaType", 0xc053_b716, offset + 4),
                ])),
            ));
        }
        Ok(Target::Table(Table::record(vec![list(
            "EvaEntry",
            0xd7b4_dedb,
            start,
            items,
        )])))
    }
    fn sound_loop(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        validate_attributes(
            node,
            &["Sound", "RequiredFlags", "ExcludedFlags"],
            self.data.path,
        )?;
        require_empty(node, self.data.path)?;
        let sound = self.optional_sound(node, start)?;
        let [required, excluded] = condition_fields(node, start + 4, self.data)?;
        Ok(Table::record(vec![sound, required, excluded]))
    }

    fn sound_override(
        &mut self,
        node: Node<'_, '_>,
        start: usize,
        upgrade: bool,
    ) -> io::Result<Table> {
        let names = if upgrade {
            ["RequiredModelConditions", "ConflictingModelConditions"]
        } else {
            ["RequiredFlags", "ExcludedFlags"]
        };
        validate_attributes(
            node,
            &[names[0], names[1], "VoicePriorityOverride"],
            self.data.path,
        )?;
        let mut voice = None;
        let mut sound = None;
        let mut required_upgrades = Vec::new();
        let mut conflicting_upgrades = Vec::new();
        for child in significant_children(node) {
            let slot = if child.has_tag_name((SAGE_NS, "AudioArrayVoice")) {
                &mut voice
            } else if child.has_tag_name((SAGE_NS, "AudioArraySound")) {
                &mut sound
            } else if upgrade && child.has_tag_name((SAGE_NS, "RequiredUpgrade")) {
                required_upgrades.push(child);
                continue;
            } else if upgrade && child.has_tag_name((SAGE_NS, "ConflictingUpgrade")) {
                conflicting_upgrades.push(child);
                continue;
            } else {
                return Err(invalid(self.data.path, "unsupported sound override child"));
            };
            if slot.replace(child).is_some() {
                return Err(invalid(self.data.path, "repeated sound override child"));
            }
        }
        let [required, excluded] = named_condition_fields(node, start, self.data, names)?;
        let priority = if let Some(value) = node.attribute("VoicePriorityOverride") {
            let value = value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(self.data.path, "invalid VoicePriorityOverride"))?;
            let target = self.data.allocate(start + 0x78, 4)?;
            self.data.word(target, value as u32);
            Target::Data(target)
        } else {
            Target::Null
        };
        let voice = self.audio_array(voice, start + 0x7c, true)?;
        let sound = self.audio_array(sound, start + 0x80, false)?;
        let mut fields = vec![
            required,
            excluded,
            Entry::named("VoicePriorityOverride", INT, priority),
            Entry::named("AudioArrayVoice", 0x09d1_8241, voice),
            Entry::named("AudioArraySound", 0x5986_7d0b, sound),
        ];
        if upgrade {
            for (name, offset, children) in [
                ("RequiredUpgrade", start + 0x84, required_upgrades),
                ("ConflictingUpgrade", start + 0x8c, conflicting_upgrades),
            ] {
                let ids = children
                    .into_iter()
                    .map(|child| {
                        super::spawn::weak_reference(
                            child,
                            "UpgradeTemplate",
                            self.data,
                            self.reference_types,
                        )
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                let ids = super::spawn::id_list(self.data, offset, ids)?;
                fields.push(Entry::named(name, 0x26fa_cfc7, Target::Table(ids)));
            }
        }
        Ok(Table::record(fields))
    }

    fn audio_array(
        &mut self,
        node: Option<Node<'_, '_>>,
        pointer: usize,
        voice: bool,
    ) -> io::Result<Target> {
        let Some(node) = node else {
            return Ok(Target::Null);
        };
        validate_attributes(node, &[], self.data.path)?;
        let mut children = Vec::new();
        let mut object_specific = Vec::new();
        let mut named = Vec::new();
        for child in significant_children(node) {
            if child.has_tag_name((SAGE_NS, "AudioEntry")) {
                children.push(child);
            } else if voice && child.has_tag_name((SAGE_NS, "ObjectSpecificEntry")) {
                object_specific.push(child);
            } else if voice && child.has_tag_name((SAGE_NS, "NamedEntry")) {
                named.push(child);
            } else {
                return Err(invalid(
                    self.data.path,
                    format!(
                        "unsupported {} child {}",
                        node.tag_name().name(),
                        child.tag_name().name()
                    ),
                ));
            }
        }
        let start = self.data.allocate(pointer, if voice { 24 } else { 8 })?;
        let size = if voice { 12 } else { 8 };
        let first = self.data.list(start, children.len(), size)?;
        let mut items = Vec::with_capacity(children.len());
        for (index, child) in children.into_iter().enumerate() {
            let offset = first + index * size;
            let table = if voice {
                self.voice_entry(child, offset)?
            } else {
                self.sound_entry(child, offset)?
            };
            items.push((0, Target::Table(table)));
        }
        let mut fields = vec![list(
            "AudioEntry",
            if voice { 0x8aa9_cb14 } else { 0x87d0_686c },
            start,
            items,
        )];
        if voice {
            let first = self.data.list(start + 8, object_specific.len(), 16)?;
            let mut items = Vec::with_capacity(object_specific.len());
            for (index, child) in object_specific.into_iter().enumerate() {
                let table = self.object_specific_voice_entry(child, first + index * 16)?;
                items.push((0, Target::Table(table)));
            }
            fields.push(list("ObjectSpecificEntry", 0xaedf_c4cd, start + 8, items));

            let first = self.data.list(start + 16, named.len(), 12)?;
            let mut items = Vec::with_capacity(named.len());
            for (index, child) in named.into_iter().enumerate() {
                let offset = first + index * 12;
                validate_attributes(child, &["Sound", "EvaEvent", "Name"], self.data.path)?;
                require_empty(child, self.data.path)?;
                let name = required(child, "Name", self.data.path)?;
                let parent = self.sound_or_eva(child, offset)?;
                self.data.word(offset + 8, fast_hash(name.as_bytes()));
                items.push((
                    0,
                    Target::Table(Table::record(vec![
                        base(0x1ae6_b6b7, parent),
                        field("Name", 0x58be_c727, offset + 8),
                    ])),
                ));
            }
            fields.push(list("NamedEntry", 0xf8ed_487b, start + 16, items));
        }
        Ok(Target::Table(Table::record(fields)))
    }

    fn voice_entry(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        validate_attributes(node, &["Sound", "EvaEvent", "AudioType"], self.data.path)?;
        require_empty(node, self.data.path)?;
        let parent = self.sound_or_eva(node, start)?;
        self.data.word(
            start + 8,
            audio_type(node, AudioType::Voice, self.data.path)?,
        );
        Ok(Table::record(vec![
            base(0x1ae6_b6b7, parent),
            field("AudioType", 0x4da7_08b0, start + 8),
        ]))
    }

    fn object_specific_voice_entry(
        &mut self,
        node: Node<'_, '_>,
        start: usize,
    ) -> io::Result<Table> {
        let path = self.data.path;
        validate_attributes(
            node,
            &["Sound", "EvaEvent", "AudioType", "TargetObject"],
            path,
        )?;
        require_empty(node, path)?;
        let target = required(node, "TargetObject", path)?;
        let target = target.split_once('\\').map_or(target, |(name, _)| name);
        let target = asset_reference(target, "GameObject", path)?;
        self.data.word(
            start + 8,
            audio_type(node, AudioType::ObjectSpecificVoice, path)?,
        );
        self.data.word(start + 12, target.instance_id);

        // EA gathers strong and weak references in XML attribute order.
        let mut sound = Entry::named("Sound", SOUND, Target::Null);
        for attribute in node.attributes() {
            match attribute.name() {
                "Sound" => sound = self.optional_sound(node, start)?,
                "EvaEvent" => {
                    let event = self.reference(attribute.value(), "EvaEvent")?;
                    self.data.word(start + 4, event);
                }
                "TargetObject" => self.reference_types.push(target.type_id),
                _ => {}
            }
        }
        let parent = Table::record(vec![sound, field("EvaEvent", 0xc4ec_43b1, start + 4)]);
        Ok(Table::record(vec![
            base(0x1ae6_b6b7, parent),
            field("AudioType", 0x94a6_9cb3, start + 8),
            field("TargetObject", 0x148e_d75d, start + 12),
        ]))
    }

    fn sound_or_eva(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        let sound = self.optional_sound(node, start)?;
        let event = self.reference(node.attribute("EvaEvent").unwrap_or(""), "EvaEvent")?;
        self.data.word(start + 4, event);
        Ok(Table::record(vec![
            sound,
            field("EvaEvent", 0xc4ec_43b1, start + 4),
        ]))
    }

    fn sound_entry(&mut self, node: Node<'_, '_>, start: usize) -> io::Result<Table> {
        validate_attributes(node, &["Sound", "AudioType"], self.data.path)?;
        require_empty(node, self.data.path)?;
        let sound = self.reference(
            node.attribute("Sound")
                .ok_or_else(|| invalid(self.data.path, "expected Sound attribute"))?,
            "BaseAudioEventInfo",
        )?;
        self.data.word(start, sound);
        self.data.word(
            start + 4,
            audio_type(node, AudioType::Sound, self.data.path)?,
        );
        Ok(Table::record(vec![
            field("Sound", SOUND, start),
            field("AudioType", 0xd50d_c497, start + 4),
        ]))
    }

    fn optional_sound(&mut self, node: Node<'_, '_>, pointer: usize) -> io::Result<Entry> {
        let target = if let Some(value) = node.attribute("Sound") {
            let ordinal = self.reference(value, "BaseAudioEventInfo")?;
            let offset = self.data.allocate(pointer, 4)?;
            self.data.word(offset, ordinal);
            Target::Data(offset)
        } else {
            Target::Null
        };
        Ok(Entry::named("Sound", SOUND, target))
    }

    fn reference(&mut self, value: &str, kind: &str) -> io::Result<u32> {
        if kind == "BaseAudioEventInfo" {
            return audio_reference(value, self.data.path, self.references, self.reference_types);
        }
        let name = value.split_once('\\').map_or(value, |(name, _)| name);
        if name.trim().is_empty() {
            return Ok(0);
        }
        self.references
            .push(asset_reference(name, kind, self.data.path)?.into());
        self.reference_types
            .push(super::super::fast_hash(kind.as_bytes()));
        Ok(self.references.len() as u32)
    }
}

pub(in crate::compile::static_assets) fn audio_reference(
    value: &str,
    path: &Path,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<u32> {
    let kind = "BaseAudioEventInfo";
    let name = value.split_once('\\').map_or(value, |(name, _)| name);
    if name.trim().is_empty() {
        return Ok(0);
    }
    let reference = match name.trim().split_once(':') {
        Some((
            kind @ ("AudioEvent" | "Multisound" | "AmbientStream" | "DialogEvent" | "MusicTrack"),
            _,
        )) => asset_reference(name, kind, path)?.into(),
        Some((_, plain)) => {
            let base = asset_reference(name, kind, path)?;
            AssetReference {
                id: asset_reference(plain, "AudioEvent", path)?,
                external_type: Some(base.type_id),
            }
        }
        None => AssetReference {
            id: asset_reference(name, "AudioEvent", path)?,
            external_type: Some(super::super::fast_hash(kind.as_bytes())),
        },
    };
    references.push(reference);
    reference_types.push(super::super::fast_hash(kind.as_bytes()));
    Ok(references.len() as u32)
}

fn condition_fields(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<[Entry; 2]> {
    named_condition_fields(node, start, data, ["RequiredFlags", "ExcludedFlags"])
}

fn named_condition_fields(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    [required, excluded]: [&str; 2],
) -> io::Result<[Entry; 2]> {
    let excluded_offset = start + 0x3c;
    write_model_conditions(node.attribute(required).unwrap_or(""), start, data)?;
    write_model_conditions(
        node.attribute(excluded).unwrap_or(""),
        excluded_offset,
        data,
    )?;
    Ok([
        field(required, 0x67db_bfc9, start),
        field(excluded, 0x67db_bfc9, excluded_offset),
    ])
}

enum AudioType {
    Voice,
    ObjectSpecificVoice,
    Sound,
    Eva,
}

fn audio_type(node: Node<'_, '_>, kind: AudioType, path: &Path) -> io::Result<u32> {
    static VOICE: OnceLock<Vec<String>> = OnceLock::new();
    static OBJECT_SPECIFIC_VOICE: OnceLock<Vec<String>> = OnceLock::new();
    static SOUND: OnceLock<Vec<String>> = OnceLock::new();
    static EVA: OnceLock<Vec<String>> = OnceLock::new();
    let sound_types = include_str!("../../../../schemas/ra3/xsd/Includes/ObjectSoundTypes.xsd");
    let (cache, schema, type_name, attribute) = match kind {
        AudioType::Voice => (&VOICE, sound_types, "ThingTemplateVoiceType", "AudioType"),
        AudioType::ObjectSpecificVoice => (
            &OBJECT_SPECIFIC_VOICE,
            sound_types,
            "ThingTemplateObjectSpecificVoiceType",
            "AudioType",
        ),
        AudioType::Sound => (&SOUND, sound_types, "ThingTemplateSoundType", "AudioType"),
        AudioType::Eva => (
            &EVA,
            include_str!("../../../../schemas/ra3/xsd/Includes/ObjectEvaTypes.xsd"),
            "ThingTemplateEvaEventType",
            "EvaType",
        ),
    };
    let values = cache.get_or_init(|| {
        Document::parse(schema)
            .unwrap()
            .descendants()
            .find(|node| node.attribute("name") == Some(type_name))
            .unwrap()
            .descendants()
            .filter(|node| node.tag_name().name() == "enumeration")
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    });
    let name = required(node, attribute, path)?;
    values
        .iter()
        .position(|value| value == name)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("invalid {type_name} {name}")))
}

fn elements<'a, 'input>(
    node: Node<'a, 'input>,
    name: &str,
    path: &Path,
) -> io::Result<Vec<Node<'a, 'input>>> {
    significant_children(node)
        .map(|child| {
            if child.has_tag_name((SAGE_NS, name)) {
                Ok(child)
            } else {
                Err(invalid(
                    path,
                    format!(
                        "unsupported {} child {}",
                        node.tag_name().name(),
                        child.tag_name().name()
                    ),
                ))
            }
        })
        .collect()
}

fn require_empty(node: Node<'_, '_>, path: &Path) -> io::Result<()> {
    if has_content(node) {
        return Err(invalid(
            path,
            format!("unexpected content in {}", node.tag_name().name()),
        ));
    }
    Ok(())
}

fn base(kind: u32, table: Table) -> Entry {
    Entry::new(1, kind, Target::Table(table))
}

fn field(name: &str, kind: u32, offset: usize) -> Entry {
    Entry::named(name, kind, Target::Data(offset))
}

fn list(
    name: &str,
    kind: u32,
    count: usize,
    items: impl IntoIterator<Item = (u32, Target)>,
) -> Entry {
    Entry::named(name, kind, Target::Table(Table::array(count, items)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::collections::HashSet;

    #[test]
    fn client_behaviors_match_ea() {
        assert_matches_ea(
            include_str!("../../../../tests/fixtures/game-object-client-behaviors.xml"),
            include_bytes!("../../../../tests/fixtures/game-object-client-behaviors.bin"),
            include_bytes!("../../../../tests/fixtures/game-object-client-behaviors.relo"),
            include_bytes!("../../../../tests/fixtures/game-object-client-behaviors.imp"),
            include_bytes!("../../../../tests/fixtures/game-object-client-behaviors.manifest"),
        );
    }

    #[test]
    fn top_level_audio_arrays_match_ea() {
        assert_matches_ea(
            include_str!("../../../../tests/fixtures/game-object-audio.xml"),
            include_bytes!("../../../../tests/fixtures/game-object-audio.bin"),
            include_bytes!("../../../../tests/fixtures/game-object-audio.relo"),
            include_bytes!("../../../../tests/fixtures/game-object-audio.imp"),
            include_bytes!("../../../../tests/fixtures/game-object-audio.manifest"),
        );
    }

    fn assert_matches_ea(xml: &str, bin: &[u8], relo: &[u8], imp: &[u8], manifest: &[u8]) {
        let document = Document::parse(xml).unwrap();
        let node = document.root_element().first_element_child().unwrap();
        let actual = compile_asset(node, Path::new("audio-oracle.xml"), "").unwrap();
        let expected = Manifest::parse(manifest).unwrap();
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
        assert_eq!(actual.imports, imp[4..]);
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.output_id(&HashSet::new()))
                .eq(expected.assets[0].references.iter())
        );
    }
}
