use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::{
    Marshaled, asset_reference, declaration_id, encode_words, sage_int, sage_time,
    significant_children, text_value, validate_attributes, write_import,
};
use crate::compile::{
    CompiledAsset, Compiler, FILE_HASH_BLOCK, SAGE_NS, escape_xml, has_content, hash_source_file,
    hash_xml, invalid, memory_stream_hash, required,
};
use crate::hash::{asset_string_hash, fast_hash_with_seed};
use roxmltree::{Document, Node};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const EVENT: &str = "PathMusicEvent";
const EVENT_HEADER: &str = "PathfinderEventHeader";
const TRACK_HEADER: &str = "PathfinderTrackHeader";

pub(super) fn palette(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "IsDefaultForNewMap"], path)?;
    let tracks: Vec<_> = significant_children(node).collect();
    let mut data = BinaryData {
        payload: vec![0; 16],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.payload[12] = boolean(node, "IsDefaultForNewMap", false, path)?;
    let start = data.list(4, tracks.len(), 4)?;
    for (index, track) in tracks.into_iter().enumerate() {
        if !track.has_tag_name((SAGE_NS, "Track")) {
            return Err(invalid(path, "expected a MusicPalette Track"));
        }
        let value = text_value(track, path)?;
        if !value.trim().is_empty() {
            write_import(
                value,
                "MusicScriptTrack",
                start + index * 4,
                &mut data,
                &mut result,
            )?;
        }
    }
    Ok(finish_music_script(result, data))
}

pub(super) fn script_track(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let flags = [
        ("ConditionsAreLatch", false),
        ("FadeInTrack", false),
        ("FadeOutLowerLevelTrack", true),
    ];
    let names = [
        "id",
        "TrackTypeKey",
        "Level",
        "Priority",
        "Condition",
        "Track",
    ];
    let names: Vec<_> = names
        .into_iter()
        .chain(flags.map(|(name, _)| name))
        .collect();
    validate_attributes(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 36],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(key) = node.attribute("TrackTypeKey").filter(|key| !key.is_empty()) {
        let key = crate::compile::windows_1252(key)
            .ok_or_else(|| invalid(path, "TrackTypeKey must use Windows-1252"))?;
        data.word(4, asset_string_hash(&key));
    }
    for (name, offset) in [("Level", 8), ("Priority", 12)] {
        data.word(offset, sage_int(required(node, name, path)?, name, path)?);
    }
    required(node, "Track", path)?;
    for attribute in node.attributes() {
        let value = attribute.value();
        let (kind, offset) = match attribute.name() {
            "Track" => ("BaseAudioEventInfo", 20),
            "Condition" => ("MusicScriptConditionNuggetBase", data.allocate(16, 4)?),
            _ => continue,
        };
        if !value.trim().is_empty() {
            write_import(value, kind, offset, &mut data, &mut result)?;
        }
    }
    let timeouts: Vec<_> = significant_children(node).collect();
    let start = data.list(24, timeouts.len(), 8)?;
    for (index, timeout) in timeouts.into_iter().enumerate() {
        if !timeout.has_tag_name((SAGE_NS, "Timeout")) {
            return Err(invalid(path, "expected a MusicScriptTrack Timeout"));
        }
        validate_leaf(timeout, &["Weight", "Duration"], path)?;
        data.word(
            start + index * 8,
            sage_int(
                timeout.attribute("Weight").unwrap_or("1000"),
                "Weight",
                path,
            )?,
        );
        data.word(
            start + index * 8 + 4,
            sage_time(required(timeout, "Duration", path)?, "Duration", path)?.to_bits(),
        );
    }
    for (index, (name, default)) in flags.into_iter().enumerate() {
        data.payload[32 + index] = boolean(node, name, default, path)?;
    }
    Ok(finish_music_script(result, data))
}

fn finish_music_script(mut result: Marshaled, mut data: BinaryData<'_>) -> Marshaled {
    for offsets in [&mut result.imports, &mut data.relocations] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    result
}

pub(in crate::compile) struct Header {
    values: HashMap<String, u32>,
    hash: u32,
}

impl Compiler<'_> {
    pub(in crate::compile) fn compile_music(
        &mut self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        if has_content(node) {
            return Err(invalid(
                path,
                "unexpected content in Pathfinder audio asset",
            ));
        }
        let (name, id) = declaration_id(node, path)?;
        let kind = node.tag_name().name();
        let mut mix = Vec::new();
        let mut relocations = Vec::new();
        let mut custom_data = None;
        let (payload, type_hash, processing_hash, sort_order, xml) = match kind {
            EVENT => {
                let (merged, inherited_header) = match self.music_event_xml(node, path)? {
                    Some((xml, header)) => (Some(xml), Some(header)),
                    None => (None, None),
                };
                let parsed = merged
                    .as_deref()
                    .map(Document::parse)
                    .transpose()
                    .map_err(|error| invalid(path, error))?;
                let effective = parsed.as_ref().map_or(node, Document::root_element);
                validate_attributes(
                    effective,
                    &["id", EVENT_HEADER, "XferAlternateEvent", "IsXferable"],
                    path,
                )?;
                let header_path = match inherited_header {
                    Some(header) => header,
                    None => self.resolve(path, required(effective, EVENT_HEADER, path)?)?,
                };
                let header = self.music_header(header_path)?;
                let event = header.get("PATH_EVENT_", name, path)?;
                let transferable = match effective.attribute("IsXferable").unwrap_or("true").trim()
                {
                    "true" | "1" => 1,
                    "false" | "0" => 0,
                    _ => return Err(invalid(path, "invalid Pathfinder IsXferable value")),
                };
                let alternate = effective
                    .attribute("XferAlternateEvent")
                    .unwrap_or("")
                    .trim();
                let mut words = vec![0, event, 0, transferable];
                if !alternate.is_empty() {
                    let alternate = asset_reference(alternate, EVENT, path)?;
                    words[2] = 16;
                    words.push(alternate.instance_id);
                    relocations.push(8);
                    mix.push(alternate.type_id);
                }
                mix.push(header.hash);
                (encode_words(words), 0x76d0_ceef, 0x76d0_cee6, 20, merged)
            }
            "PathMusicMap" => {
                validate_attributes(node, &["id", "File"], path)?;
                let file = self.resolve(path, required(node, "File", path)?)?;
                let bytes = fs::read(&file).map_err(|error| invalid(&file, error))?;
                let length = u32::try_from(bytes.len())
                    .map_err(|_| invalid(&file, "Pathfinder map is too large"))?;
                mix.push(hash_bytes(&bytes, length));
                let mut payload = encode_words([0, 12, length]);
                payload.extend_from_slice(&bytes);
                payload.resize(payload.len().next_multiple_of(4), 0);
                relocations.push(4);
                (payload, 0xba1b_4ccb, 0x1151_6995, 21, None)
            }
            "PathMusicTrack" => {
                validate_attributes(node, &["id", "File", "Type", TRACK_HEADER], path)?;
                let file = self.resolve(path, required(node, "File", path)?)?;
                let header_path = self.resolve(path, required(node, TRACK_HEADER, path)?)?;
                let header = self.music_header(header_path.clone())?;
                let handle = header.get("PATH_TRACK_", name, path)?;
                let track_type = match required(node, "Type", path)? {
                    "MAIN" => 0,
                    "OVERLAY_0" => 1,
                    _ => return Err(invalid(path, "invalid Pathfinder track type")),
                };
                let track_hash = hash_source_file(&file, io::sink())?;
                for attribute in node.attributes() {
                    match attribute.name() {
                        "File" => mix.push(track_hash),
                        TRACK_HEADER => mix.push(header.hash),
                        _ => {}
                    }
                }
                let mut payload = encode_words([0, 0, 0, track_type, 0, 0, handle]);
                append_path(&mut payload, 4, &file)?;
                append_path(&mut payload, 16, &header_path)?;
                custom_data = Some(crate::compile::CustomData::Source(file, track_hash));
                relocations.extend([8, 20]);
                (payload, 0x9f12_cf33, 0x9f12_cf3a, 22, None)
            }
            _ => unreachable!("unsupported Pathfinder asset"),
        };
        let effective = xml
            .as_deref()
            .map(Document::parse)
            .transpose()
            .map_err(|error| invalid(path, error))?;
        let node = effective.as_ref().map_or(node, Document::root_element);
        let instance_hash =
            hash_xml(node, processing_hash, path)? ^ memory_stream_hash(encode_words(mix));
        if !relocations.is_empty() {
            relocations.push(u32::MAX);
        }
        Ok(CompiledAsset {
            id,
            type_hash,
            instance_hash,
            name: format!("{kind}:{name}"),
            source: source.to_owned(),
            payload,
            relocations: encode_words(relocations),
            imports: Vec::new(),
            references: Vec::new(),
            weak_references: Vec::new(),
            sort_order,
            custom_data,
            tokenized: false,
        })
    }

    fn music_event_xml(
        &self,
        node: Node<'_, '_>,
        path: &Path,
    ) -> io::Result<Option<(String, PathBuf)>> {
        let Some(base) = node.attribute("inheritFrom") else {
            return Ok(None);
        };
        let id = asset_reference(base, EVENT, path)?;
        let Some([candidate]) = self.tentative.get(&id).map(Vec::as_slice) else {
            return Err(invalid(
                path,
                format!("expected one instance include for {base}"),
            ));
        };
        let doc =
            Document::parse(&candidate.xml).map_err(|error| invalid(&candidate.path, error))?;
        let parent = doc
            .get_node(candidate.node)
            .ok_or_else(|| invalid(path, "missing Pathfinder event base"))?;
        validate_attributes(
            parent,
            &["id", EVENT_HEADER, "XferAlternateEvent", "IsXferable"],
            &candidate.path,
        )?;
        validate_attributes(
            node,
            &[
                "id",
                "inheritFrom",
                EVENT_HEADER,
                "XferAlternateEvent",
                "IsXferable",
            ],
            path,
        )?;
        if has_content(parent) {
            return Err(invalid(path, "unexpected content in Pathfinder event base"));
        }
        let header = match node.attribute(EVENT_HEADER) {
            Some(file) => self.resolve(path, file)?,
            None => self.resolve(
                &candidate.path,
                required(parent, EVENT_HEADER, &candidate.path)?,
            )?,
        };
        let inherited_path = sdk_path(&header)?;
        let mut xml = format!("<PathMusicEvent xmlns=\"{SAGE_NS}\"");
        for attribute in parent.attributes() {
            let name = attribute.name();
            let inherited = if name == EVENT_HEADER {
                &inherited_path
            } else {
                attribute.value()
            };
            append_attribute(&mut xml, name, node.attribute(name).unwrap_or(inherited));
        }
        if parent.attribute("IsXferable").is_none() {
            append_attribute(
                &mut xml,
                "IsXferable",
                node.attribute("IsXferable").unwrap_or("true"),
            );
        }
        for attribute in node.attributes().filter(|attribute| {
            !matches!(attribute.name(), "inheritFrom" | "IsXferable")
                && parent.attribute(attribute.name()).is_none()
        }) {
            append_attribute(&mut xml, attribute.name(), attribute.value());
        }
        xml.push_str(" />");
        Ok(Some((xml, header)))
    }

    fn music_header(&mut self, path: PathBuf) -> io::Result<&Header> {
        use std::collections::hash_map::Entry;
        match self.music_headers.entry(path) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let path = entry.key();
                let bytes = fs::read(path).map_err(|error| invalid(path, error))?;
                let length = u32::try_from(bytes.len())
                    .map_err(|_| invalid(path, "Pathfinder header is too large"))?;
                let text = std::str::from_utf8(&bytes).map_err(|error| invalid(path, error))?;
                let mut values = HashMap::new();
                for line in text.lines() {
                    let mut words = line.split_ascii_whitespace();
                    if let (Some("#define"), Some(key), Some(value)) =
                        (words.next(), words.next(), words.next())
                        && (key.starts_with("PATH_EVENT_") || key.starts_with("PATH_TRACK_"))
                    {
                        let value = value.strip_prefix("0x").ok_or_else(|| {
                            invalid(path, "expected a hexadecimal Pathfinder handle")
                        })?;
                        let value =
                            u32::from_str_radix(value, 16).map_err(|error| invalid(path, error))?;
                        values.insert(key.to_ascii_lowercase(), value);
                    }
                }
                Ok(entry.insert(Header {
                    values,
                    hash: hash_bytes(&bytes, length),
                }))
            }
        }
    }
}

impl Header {
    fn get(&self, prefix: &str, name: &str, path: &Path) -> io::Result<u32> {
        self.values
            .get(&format!("{prefix}{name}").to_ascii_lowercase())
            .copied()
            .ok_or_else(|| invalid(path, format!("Pathfinder header has no {prefix}{name}")))
    }
}

pub(super) fn event_set(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Category"], path)?;
    let category = match required(node, "Category", path)? {
        "SHELL" => 0,
        "SCRIPT" => 1,
        _ => return Err(invalid(path, "invalid Pathfinder event set category")),
    };
    let mut data = Marshaled::default();
    let mut words = vec![0, category, 0, 0];
    for event in significant_children(node) {
        if !event.has_tag_name((SAGE_NS, "Event")) {
            return Err(invalid(path, "expected an Event in PathMusicEventSet"));
        }
        let id = asset_reference(text_value(event, path)?, EVENT, path)?;
        data.references.push(id.into());
        data.reference_types.push(id.type_id);
        data.imports.push((4 * words.len()) as u32);
        words.push(data.references.len() as u32);
    }
    if !data.references.is_empty() {
        words[2] = data.references.len() as u32;
        words[3] = 16;
        data.relocations.extend([12, u32::MAX]);
        data.imports.push(u32::MAX);
    }
    data.payload = encode_words(words);
    Ok(data)
}

fn append_attribute(xml: &mut String, name: &str, value: &str) {
    xml.push(' ');
    xml.push_str(name);
    xml.push_str("=\"");
    escape_xml(value, true, xml);
    xml.push('"');
}

fn append_path(payload: &mut Vec<u8>, field: usize, path: &Path) -> io::Result<()> {
    let text = sdk_path(path)?;
    let offset = payload.len() as u32;
    payload[field..field + 4].copy_from_slice(&(text.len() as u32).to_le_bytes());
    payload[field + 4..field + 8].copy_from_slice(&offset.to_le_bytes());
    payload.extend_from_slice(text.as_bytes());
    payload.resize((payload.len() + 1).next_multiple_of(4), 0);
    Ok(())
}

fn sdk_path(path: &Path) -> io::Result<String> {
    let text = path
        .to_str()
        .filter(|path| path.is_ascii())
        .ok_or_else(|| invalid(path, "Pathfinder file path must be ASCII"))?;
    Ok(text
        .strip_prefix(r"\\?\")
        .unwrap_or(text)
        .replace('/', "\\")
        .to_ascii_lowercase())
}

fn hash_bytes(bytes: &[u8], length: u32) -> u32 {
    bytes
        .chunks(FILE_HASH_BLOCK)
        .fold(length, fast_hash_with_seed)
}
