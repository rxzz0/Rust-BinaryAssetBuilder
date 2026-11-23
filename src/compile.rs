//! Compiles supported Red Alert 3 PC XML streams.

use crate::hash::{fast_hash, fast_hash_with_seed};
use crate::linked_stream::LinkedFiles;
use crate::manifest::{
    AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest, ReferencedManifest,
};
use crate::sdk_references::{AUDIO_BASE, AUDIO_PATCH, is_stock_audio_catalog};
use expressions::{Definitions, expand_assets, merge_definitions, read_definitions};
use roxmltree::{Document, Node, NodeId};
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, mpsc};

use asset_workers::Encoders;

pub(crate) mod asset_workers;
mod coverage;
mod expressions;
mod merge_assets;
mod precompiled;
mod sdk_maps;
mod sdk_static;
mod static_assets;
mod stream_order;
mod string_hash_stream;
mod string_hashes;
mod terrain_atlas;
mod texture;
mod xinclude;

pub use crate::compile_batch::{compile_apt_ui_directory, write_compiled_stream};
pub use coverage::{sdk_coverage, verify_ea_batch};
pub use merge_assets::merge_assets;
pub use sdk_maps::{compile_sdk_map_overrides, compile_sdk_maps};
pub use sdk_static::{
    compile_sdk_global, compile_sdk_static, compile_sdk_static_with_string_hashes,
};

const SAGE_NS: &str = "uri:ea.com:eala:asset";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const FILE_HASH_BLOCK: usize = 0x100000;
pub(crate) const RA3_ALL_TYPES_HASH: u32 = 0x54ee_e764;
const RELOCATIONS: [u8; 8] = [4, 0, 0, 0, 255, 255, 255, 255];

pub struct CompiledStream {
    pub manifest: Vec<u8>,
    pub files: LinkedFiles,
    pub custom_data: Vec<(String, CustomData)>,
}

pub enum CustomData {
    Source(PathBuf, u32),
    Bytes(Vec<u8>),
}

/// Compiles a linked RA3 PC stream from AptUI declarations and their includes.
pub fn compile_apt_ui(input: &Path, data_root: &Path) -> io::Result<CompiledStream> {
    compile(input, data_root, None, ReferenceMode::Explicit)
}

/// Compiles supported static XML assets against the SDK's precompiled streams.
pub fn compile_static(
    input: &Path,
    data_root: &Path,
    reference_directory: &Path,
) -> io::Result<CompiledStream> {
    compile(
        input,
        data_root,
        Some(reference_directory),
        ReferenceMode::Explicit,
    )
}

/// Compiles an RA3 map stream linked against the SDK's WorldBuilder assets.
pub fn compile_map(
    input: &Path,
    data_root: &Path,
    reference_directory: &Path,
) -> io::Result<CompiledStream> {
    compile(
        input,
        data_root,
        Some(reference_directory),
        ReferenceMode::Map,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceMode {
    Explicit,
    SdkStatic,
    Implicit,
    Map,
}

const SDK_BASE_MANIFESTS: [&str; 3] = ["audio.manifest", "global.manifest", "static.manifest"];

fn sdk_manifest_path(directory: &Path, relative: &Path, name: &str) -> PathBuf {
    let path = directory.join(relative);
    if SDK_BASE_MANIFESTS.contains(&name) && !path.is_file() {
        let nested = directory.join("sagexml").join(relative);
        if nested.is_file() {
            return nested;
        }
    }
    path
}

impl ReferenceMode {
    fn validates_schema(self) -> bool {
        self != Self::Map
    }
}

fn compile(
    input: &Path,
    data_root: &Path,
    reference_directory: Option<&Path>,
    reference_mode: ReferenceMode,
) -> io::Result<CompiledStream> {
    compile_with_source_references(input, data_root, reference_directory, reference_mode, None)
        .map(|(stream, _)| stream)
}

fn compile_with_source_references(
    input: &Path,
    data_root: &Path,
    reference_directory: Option<&Path>,
    reference_mode: ReferenceMode,
    source_references: Option<&mut sdk_static::SourceReferences>,
) -> io::Result<(CompiledStream, HashSet<AssetId>)> {
    let root = fs::canonicalize(data_root)?;
    let resolved_input = fs::canonicalize(input)?;
    let redirected_apt = if reference_directory.is_none() && !resolved_input.starts_with(&root) {
        redirected_apt_ui(input, data_root, &resolved_input)
    } else {
        None
    };
    let mod_root =
        sdk_relative_path(&resolved_input, &root, redirected_apt.as_ref()).and_then(|relative| {
            let mut components = relative.iter();
            let mods = components.next()?;
            let name = components.next()?;
            mods.eq_ignore_ascii_case("mods")
                .then(|| root.join(mods).join(name))
        });
    let is_map = reference_mode == ReferenceMode::Map;
    let map_root = if is_map {
        resolved_input.parent()
    } else {
        None
    };
    let paths = |directory, sdk_directory| {
        search_paths(
            &root,
            map_root,
            mod_root.as_deref(),
            directory,
            sdk_directory,
        )
    };
    let worldbuilder_hashes = if is_map {
        crate::fix_assets::worldbuilder_hashes(reference_directory.unwrap())?
    } else {
        HashMap::new()
    };
    let schema_root = root.join("Schemas/xsd");
    let game_object_schema =
        if reference_directory.is_some() && schema_root.join("CnC3Types.xsd").try_exists()? {
            Some(static_assets::game_object::Schema::load(&schema_root)?)
        } else {
            None
        };
    let mut compiler = Compiler {
        data_paths: paths("Data", "SageXml"),
        art_paths: paths("Art", "Art"),
        audio_paths: paths("Audio", "Audio"),
        map_art_path: map_root.map(|map| map.join("Art")),
        root,
        redirected_apt,
        visited: HashSet::new(),
        active: HashSet::new(),
        ids: HashSet::new(),
        static_asset_locations: HashMap::new(),
        next_document_order: 0,
        encoders: if reference_mode == ReferenceMode::SdkStatic
            && source_references
                .as_ref()
                .is_none_or(|references| references.is_root())
        {
            Encoders::start()?
        } else {
            None
        },
        pending: VecDeque::new(),
        local_assets: HashSet::new(),
        asset_document: None,
        assets: Vec::new(),
        tentative: HashMap::new(),
        map_all_includes: HashMap::new(),
        map_source: map_root.map(|_| resolved_input.clone()),
        definitions: HashMap::new(),
        xml_fragments: HashMap::new(),
        game_object_schema,
        music_headers: HashMap::new(),
        reference_directory,
        reference_mode,
        source_references,
        referenced_manifests: Vec::new(),
        external_ids: HashSet::new(),
        worldbuilder_hashes,
    };
    let traversal = compiler.read_document(&resolved_input, false, false);
    while !compiler.pending.is_empty() {
        compiler.commit_pending()?;
    }
    traversal?;
    compiler.add_referenced_instances()?;
    compiler.finish()
}

struct RedirectedAptUi {
    sdk_relative: PathBuf,
    physical: PathBuf,
}

fn redirected_apt_ui(input: &Path, data_root: &Path, resolved: &Path) -> Option<RedirectedAptUi> {
    let input = std::path::absolute(input).ok()?;
    let data_root = std::path::absolute(data_root).ok()?;
    let relative = input.strip_prefix(&data_root).ok()?;
    if !relative
        .components()
        .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return None;
    }
    let mut parts = relative.iter();
    let (mods, name, data, apt) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if !mods.eq_ignore_ascii_case("Mods")
        || !data.eq_ignore_ascii_case("Data")
        || !apt.eq_ignore_ascii_case("AptUI")
        || parts.next().is_none()
    {
        return None;
    }
    let sdk_relative = PathBuf::from(mods).join(name).join(data).join(apt);
    // A directory redirect is allowed for this mod's AptUI tree. Its XML and
    // asset files must still resolve inside that tree or the canonical SDK.
    let physical = fs::canonicalize(data_root.join(&sdk_relative)).ok()?;
    resolved.starts_with(&physical).then_some(RedirectedAptUi {
        sdk_relative,
        physical,
    })
}

fn sdk_relative_path<'a>(
    path: &'a Path,
    root: &Path,
    redirected_apt: Option<&RedirectedAptUi>,
) -> Option<Cow<'a, Path>> {
    if let Ok(relative) = path.strip_prefix(root) {
        return Some(Cow::Borrowed(relative));
    }
    let redirected = redirected_apt?;
    let relative = path.strip_prefix(&redirected.physical).ok()?;
    Some(Cow::Owned(redirected.sdk_relative.join(relative)))
}

fn search_paths(
    root: &Path,
    map_root: Option<&Path>,
    mod_root: Option<&Path>,
    directory: &str,
    sdk_directory: &str,
) -> Vec<PathBuf> {
    let mut paths = vec![root.to_path_buf()];
    if let Some(map_root) = map_root {
        paths.push(map_root.join(directory));
    }
    if let Some(mod_root) = mod_root {
        paths.push(mod_root.join(directory));
    }
    paths.extend([root.join("Mods"), root.join(sdk_directory)]);
    paths
}

struct Compiler<'a> {
    root: PathBuf,
    redirected_apt: Option<RedirectedAptUi>,
    data_paths: Vec<PathBuf>,
    art_paths: Vec<PathBuf>,
    audio_paths: Vec<PathBuf>,
    visited: HashSet<(PathBuf, bool)>,
    active: HashSet<PathBuf>,
    ids: HashSet<AssetId>,
    static_asset_locations: HashMap<AssetId, (usize, usize)>,
    next_document_order: usize,
    encoders: Option<Encoders>,
    pending: VecDeque<PendingAsset>,
    local_assets: HashSet<AssetId>,
    asset_document: Option<usize>,
    assets: Vec<CompiledAsset>,
    tentative: HashMap<AssetId, Vec<TentativeAsset>>,
    map_all_includes: HashMap<PathBuf, Vec<PathBuf>>,
    map_source: Option<PathBuf>,
    definitions: HashMap<PathBuf, Rc<Definitions>>,
    xml_fragments: HashMap<PathBuf, Rc<str>>,
    game_object_schema: Option<static_assets::game_object::Schema>,
    music_headers: HashMap<PathBuf, static_assets::music::Header>,
    reference_directory: Option<&'a Path>,
    source_references: Option<&'a mut sdk_static::SourceReferences>,
    reference_mode: ReferenceMode,
    map_art_path: Option<PathBuf>,
    referenced_manifests: Vec<String>,
    external_ids: HashSet<AssetId>,
    worldbuilder_hashes: HashMap<AssetId, u32>,
}

struct PendingAsset {
    path: PathBuf,
    document_order: usize,
    result: AssetResult,
}

enum AssetResult {
    Ready(io::Result<Box<CompiledAsset>>),
    Running(mpsc::Receiver<io::Result<CompiledAsset>>),
}

#[derive(Clone)]
struct TentativeAsset {
    path: PathBuf,
    source: Rc<str>,
    xml: Rc<str>,
    node: NodeId,
    order: usize,
    same_id_override: bool,
}

struct CompiledAsset {
    id: AssetId,
    type_hash: u32,
    instance_hash: u32,
    name: String,
    source: String,
    payload: Vec<u8>,
    relocations: Vec<u8>,
    imports: Vec<u8>,
    references: Vec<AssetReference>,
    weak_references: Vec<AssetReference>,
    custom_data: Option<CustomData>,
    tokenized: bool,
    sort_order: u8,
}

#[derive(PartialEq, Eq)]
struct AssetReference {
    id: AssetId,
    external_type: Option<u32>,
    name: Option<Box<str>>,
}

impl From<AssetId> for AssetReference {
    fn from(id: AssetId) -> Self {
        Self {
            id,
            external_type: None,
            name: None,
        }
    }
}

const BASE_ASSET_TYPE: u32 = 0x50c6_8ad4;
const AUDIO_ASSET_TYPE: u32 = 0x4053_c714;
const RENDER_ASSET_TYPE: u32 = 0xb22f_87c9;

impl AssetReference {
    fn candidate_ids(&self) -> impl Iterator<Item = AssetId> + '_ {
        use static_assets::{
            conquer_cards, music_conditions, subsystem_settings, targeting_compare,
        };
        static TARGETING_COMPARE_TYPE: std::sync::LazyLock<u32> =
            std::sync::LazyLock::new(|| fast_hash(targeting_compare::BASE.as_bytes()));
        static SUBSYSTEM_SETTINGS_TYPE: std::sync::LazyLock<u32> =
            std::sync::LazyLock::new(|| fast_hash(subsystem_settings::BASE.as_bytes()));
        static CONQUER_CARD_TYPE: std::sync::LazyLock<u32> =
            std::sync::LazyLock::new(|| fast_hash(conquer_cards::BASE.as_bytes()));
        let subtypes: &[&str] = match (self.id.type_id, self.external_type) {
            (RENDER_ASSET_TYPE, _) => &["W3DContainer", "W3DMesh", "W3DCollisionBox"],
            (AUDIO_ASSET_TYPE, _) | (_, Some(AUDIO_ASSET_TYPE)) => &[
                "AudioEvent",
                "AudioEventOverridable",
                "Multisound",
                "AmbientStream",
                "DialogEvent",
                "MusicTrack",
                "MP3AmbientStream",
                "MP3DialogEvent",
                "MP3MusicTrack",
            ],
            (kind, _) if kind == *TARGETING_COMPARE_TYPE => targeting_compare::TYPES,
            (kind, _) if kind == *SUBSYSTEM_SETTINGS_TYPE => subsystem_settings::TYPES,
            (kind, _) if kind == *CONQUER_CARD_TYPE => conquer_cards::TYPES,
            _ => &[],
        };
        static MUSIC_CONDITION_TYPE: std::sync::LazyLock<u32> =
            std::sync::LazyLock::new(|| fast_hash(music_conditions::BASE.as_bytes()));
        let music_subtypes = (self.id.type_id == *MUSIC_CONDITION_TYPE)
            .then(music_conditions::types)
            .into_iter()
            .flatten();
        std::iter::once(self.id).chain(subtypes.iter().copied().chain(music_subtypes).map(|name| {
            AssetId {
                type_id: fast_hash(name.as_bytes()),
                instance_id: self.id.instance_id,
            }
        }))
    }

    fn local_id(&self, ids: &HashSet<AssetId>) -> Option<AssetId> {
        if self.id.type_id == BASE_ASSET_TYPE {
            let mut matches = ids
                .iter()
                .filter(|id| id.instance_id == self.id.instance_id);
            let first = matches.next().copied()?;
            return matches.next().is_none().then_some(first);
        }
        self.candidate_ids().find(|id| ids.contains(id))
    }

    fn external_id(&self, ids: &HashSet<AssetId>) -> Option<AssetId> {
        self.local_id(ids).or_else(|| {
            let declared = self.declared_id();
            ids.contains(&declared).then_some(declared)
        })
    }

    fn declared_id(&self) -> AssetId {
        AssetId {
            type_id: self.external_type.unwrap_or(self.id.type_id),
            instance_id: self.id.instance_id,
        }
    }

    fn diagnostic_name(&self) -> std::borrow::Cow<'_, str> {
        match &self.name {
            Some(name) => std::borrow::Cow::Borrowed(name),
            None => {
                let id = self.declared_id();
                format!("{:08x}:{:08x}", id.type_id, id.instance_id).into()
            }
        }
    }

    fn output_id(&self, local_ids: &HashSet<AssetId>) -> AssetId {
        self.local_id(local_ids)
            .unwrap_or_else(|| self.declared_id())
    }
}

impl Compiler<'_> {
    fn read_document(&mut self, path: &Path, tentative: bool, art_include: bool) -> io::Result<()> {
        if self.active.contains(path) {
            return Err(invalid(path, "include cycle"));
        }
        if self.active.len() >= 128 {
            return Err(invalid(path, "includes are nested more than 128 levels"));
        }
        if !self.visited.insert((path.to_path_buf(), tentative)) {
            return Ok(());
        }
        let document_order = self.next_document_order;
        self.next_document_order += 1;
        self.active.insert(path.to_path_buf());
        let relative = sdk_relative_path(path, &self.root, self.redirected_apt.as_ref())
            .ok_or_else(|| invalid(path, "XML lies outside the data root"))?;
        let relative = relative
            .to_str()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| invalid(path, "source path must be ASCII"))?;
        let source: Rc<str> =
            format!("DATA:{}", relative.replace('\\', "/").to_ascii_lowercase()).into();
        let xml: Rc<str> = if self.reference_directory.is_some() {
            self.read_static_xml(path)?
        } else {
            read_with_context(path, fs::read_to_string)?.into()
        };
        let document = Document::parse(&xml).map_err(|error| invalid(path, error))?;
        let root = document.root_element();
        if self.reference_directory.is_some()
            && path.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("w3x") || art_include && ext.eq_ignore_ascii_case("xml")
            })
            && root.tag_name().namespace().is_none()
            && root.has_tag_name("AssetDeclaration")
            && root.attributes().len() == 0
            && !has_content(root)
        {
            // EA accepts empty namespace-free W3X and ART XML placeholders.
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
            {
                // Acceptance depends on the include alias, not just the resolved file.
                self.visited.remove(&(path.to_path_buf(), tentative));
            }
            self.active.remove(path);
            return Ok(());
        }
        if !root.has_tag_name((SAGE_NS, "AssetDeclaration")) || root.attributes().len() != 0 {
            return Err(invalid(path, "expected a SAGE AssetDeclaration root"));
        }
        let mut definitions = Rc::default();
        let expanded = if self.reference_directory.is_some() {
            for node in root
                .children()
                .filter(|node| node.has_tag_name((SAGE_NS, "Includes")))
            {
                self.read_includes(node, path, tentative, &mut definitions)?;
            }
            for node in root
                .children()
                .filter(|node| node.has_tag_name((SAGE_NS, "Defines")))
            {
                read_definitions(node, &mut definitions, path)?;
            }
            let expanded = expand_assets(root, &definitions, path)?;
            if !definitions.is_empty() {
                self.definitions
                    .insert(path.to_path_buf(), definitions.clone());
            }
            expanded
        } else {
            None
        };
        let expanded_document = expanded
            .as_ref()
            .map(|xml| Document::parse(xml))
            .transpose()
            .map_err(|error| invalid(path, error))?;
        let root = expanded_document
            .as_ref()
            .map_or(root, Document::root_element);
        let xml = expanded.as_ref().unwrap_or(&xml);
        let mut shared_xml = None;
        for node in root.children().filter(Node::is_element) {
            if node.has_tag_name((SAGE_NS, "Includes")) {
                if self.reference_directory.is_none() {
                    self.read_includes(node, path, tentative, &mut definitions)?;
                }
            } else if self.reference_directory.is_some()
                && (node.has_tag_name((SAGE_NS, "Defines"))
                    || node.has_tag_name((SAGE_NS, "Tags"))
                        && node.attributes().len() == 0
                        && !has_content(node))
            {
                continue;
            } else {
                if let Some(hashes) = self
                    .source_references
                    .as_deref_mut()
                    .and_then(|references| references.string_hashes.as_mut())
                {
                    let (id, _) = static_assets::declaration_id(node, path)?;
                    hashes.asset(node.tag_name().name(), id, path)?;
                    if let Some(schema) = &self.game_object_schema {
                        schema.collect_string_hashes(node, node.tag_name().name(), path, hashes)?;
                    }
                }
                if tentative || self.reference_mode == ReferenceMode::Map {
                    let (_, id) = static_assets::declaration_id(node, path)?;
                    let same_id_override = node.attribute("inheritFrom").is_some_and(|parent| {
                        static_assets::inheritance_id(node, parent, path)
                            .is_ok_and(|parent| parent == id)
                    });
                    self.tentative.entry(id).or_default().push(TentativeAsset {
                        path: path.to_path_buf(),
                        source: Rc::clone(&source),
                        xml: Rc::clone(xml),
                        node: node.id(),
                        order: document_order,
                        same_id_override,
                    });
                }
                if tentative {
                    continue;
                }
                if let Some(encoders) = &self.encoders {
                    if self.pending.len() >= encoders.capacity()
                        && let Err(error) = self.commit_pending()
                    {
                        self.pending.clear();
                        return Err(error);
                    }
                    let result = self.queue_asset(node, path, &source, xml, &mut shared_xml)?;
                    self.pending.push_back(PendingAsset {
                        path: path.to_path_buf(),
                        document_order,
                        result,
                    });
                } else {
                    let asset = if self.reference_directory.is_some() {
                        self.compile_static_node(node, path, &source)?
                    } else {
                        self.compile_apt_asset(node, path, &source)?
                    };
                    self.commit_asset(asset, path, document_order)?;
                }
            }
        }
        self.active.remove(path);
        Ok(())
    }

    fn queue_asset(
        &mut self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
        xml: &str,
        shared_xml: &mut Option<Arc<str>>,
    ) -> io::Result<AssetResult> {
        let kind = node.tag_name().name();
        if !matches!(kind, "AudioFile" | "W3DAnimation") || node.attribute("inheritFrom").is_some()
        {
            return Ok(AssetResult::Ready(
                self.compile_static_node(node, path, source).map(Box::new),
            ));
        }

        let cleaned = self.schema_cleaned_xml(node, path)?;
        let encoders = self.encoders.as_ref().unwrap();
        let path = path.to_path_buf();
        let source = source.to_owned();
        let result = if kind == "AudioFile" {
            let document = reparse_cleaned(cleaned.as_deref(), &path)?;
            let input = self.prepare_audio_file(cleaned_node(node, document.as_ref()), &path)?;
            encoders.submit(move || input.encode(&path, &source))?
        } else {
            let position = node.range().start;
            let node_id = node.id();
            let xml = cleaned
                .is_none()
                .then(|| Arc::clone(shared_xml.get_or_insert_with(|| xml.into())));
            encoders.submit(move || {
                let text = cleaned.as_deref().or(xml.as_deref()).unwrap();
                let doc = Document::parse(text).map_err(|error| invalid(&path, error))?;
                let node = if cleaned.is_some() {
                    doc.descendants()
                        .find(|node| node.is_element() && node.range().start == position)
                } else {
                    doc.get_node(node_id)
                }
                .ok_or_else(|| invalid(&path, "missing animation declaration"))?;
                static_assets::compile_asset(node, &path, &source)
            })?
        };
        Ok(AssetResult::Running(result))
    }

    fn commit_pending(&mut self) -> io::Result<()> {
        let pending = self.pending.pop_front().unwrap();
        let asset = match pending.result {
            AssetResult::Ready(asset) => *asset?,
            AssetResult::Running(asset) => asset
                .recv()
                .map_err(|error| invalid(&pending.path, error))??,
        };
        self.commit_asset(asset, &pending.path, pending.document_order)
    }

    fn commit_asset(
        &mut self,
        asset: CompiledAsset,
        path: &Path,
        document_order: usize,
    ) -> io::Result<()> {
        if self.reference_directory.is_some() {
            if self.asset_document != Some(document_order) {
                self.local_assets.clear();
                self.asset_document = Some(document_order);
            }
            if !self.local_assets.insert(asset.id) {
                return Err(invalid(path, format!("duplicate asset ID {}", asset.name)));
            }
            if let Some((index, previous_order)) = self.static_asset_locations.get_mut(&asset.id) {
                // An including document wins over descendants. The first sibling wins.
                if document_order < *previous_order {
                    self.assets[*index] = asset;
                    *previous_order = document_order;
                }
                return Ok(());
            }
            self.static_asset_locations
                .insert(asset.id, (self.assets.len(), document_order));
        }
        if !self.ids.insert(asset.id) {
            return Err(invalid(path, format!("duplicate asset ID {}", asset.name)));
        }
        self.assets.push(asset);
        Ok(())
    }

    fn read_includes(
        &mut self,
        node: Node<'_, '_>,
        path: &Path,
        tentative: bool,
        definitions: &mut Rc<Definitions>,
    ) -> io::Result<()> {
        if node.attributes().len() != 0 {
            return Err(invalid(path, "unsupported Includes attributes"));
        }
        for include in node.children().filter(Node::is_element) {
            if !include.has_tag_name((SAGE_NS, "Include"))
                || include.attributes().len() != 2
                || has_content(include)
            {
                return Err(invalid(path, "invalid Include element"));
            }
            let source = required(include, "source", path)?;
            let kind = required(include, "type", path)?;
            let included_tentative = match kind {
                "all" => tentative,
                "instance" if self.reference_directory.is_some() => true,
                "reference" if self.reference_directory.is_some() && !tentative => {
                    self.read_reference(path, source)?;
                    continue;
                }
                _ => return Err(invalid(path, format!("unsupported include type {kind:?}"))),
            };
            let art_include = source
                .split_once(':')
                .is_some_and(|(alias, _)| alias.trim_start().eq_ignore_ascii_case("art"));
            let target = match self.resolve(path, source) {
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        && self.reference_mode == ReferenceMode::SdkStatic
                        && matches!(kind, "all" | "instance") =>
                {
                    let diagnostic = format!(
                        "Error: Input file '{}' not found (referenced from '{}'). Treating it as empty.\n",
                        source,
                        path.display()
                    );
                    write_warnings(diagnostic.as_bytes());
                    continue;
                }
                Err(error) if self.reference_mode == ReferenceMode::Map => {
                    return Err(self.map_include_error(path, source, error));
                }
                result => result?,
            };
            if self.reference_mode == ReferenceMode::Map && kind == "all" {
                self.map_all_includes
                    .entry(path.to_path_buf())
                    .or_default()
                    .push(target.clone());
            }
            self.read_document(&target, included_tentative, art_include)?;
            if let Some(included) = self.definitions.get(&target) {
                merge_definitions(definitions, included, path)?;
            }
        }
        Ok(())
    }

    fn map_include_error(&self, path: &Path, source: &str, error: io::Error) -> io::Error {
        let map = self.map_source.as_deref().unwrap_or(path);
        io::Error::new(
            error.kind(),
            format!(
                "{}: include {source:?} from {}: {error}",
                map.display(),
                path.display()
            ),
        )
    }

    fn map_candidate<'a>(
        candidates: impl Iterator<Item = &'a TentativeAsset>,
    ) -> Option<&'a TentativeAsset> {
        candidates.min_by_key(|candidate| (!candidate.same_id_override, candidate.order))
    }

    fn add_referenced_instances(&mut self) -> io::Result<()> {
        if self.tentative.is_empty() {
            return Ok(());
        }
        let mut index = 0;
        let mut pending = Vec::new();
        while index < self.assets.len() {
            let asset = &self.assets[index];
            let weak = asset
                .weak_references
                .iter()
                .filter(|_| self.reference_mode == ReferenceMode::Map);
            for reference in asset.references.iter().chain(weak) {
                if reference.local_id(&self.ids).is_some() {
                    continue;
                }
                if self.reference_mode == ReferenceMode::Map
                    && reference.id.type_id == BASE_ASSET_TYPE
                {
                    let mut matches = self
                        .tentative
                        .keys()
                        .copied()
                        .filter(|id| id.instance_id == reference.id.instance_id);
                    if let Some(id) = matches.next() {
                        if matches.next().is_some() {
                            return Err(invalid(
                                &self.root,
                                format!(
                                    "ambiguous instance reference {:08x} in {}",
                                    reference.id.instance_id, asset.name
                                ),
                            ));
                        }
                        pending.push(id);
                    }
                } else {
                    pending.extend(
                        reference
                            .candidate_ids()
                            .find(|id| self.tentative.contains_key(id)),
                    );
                }
            }
            for id in pending.drain(..) {
                if self.ids.contains(&id) {
                    continue;
                }
                // A stock manifest already supplies this instance for SDK builds.
                if self.reference_mode == ReferenceMode::SdkStatic
                    && self.external_ids.contains(&id)
                {
                    continue;
                }
                let Some(candidates) = self.tentative.get(&id) else {
                    continue;
                };
                let candidate = match candidates.as_slice() {
                    [candidate] => Some(candidate),
                    candidates if self.reference_mode == ReferenceMode::Map => {
                        // Keep include order for repeated base declarations. Explicit
                        // same-ID overrides precede the declarations they replace.
                        Self::map_candidate(candidates.iter()).filter(|selected| {
                            candidates
                                .iter()
                                .filter(|candidate| candidate.order == selected.order)
                                .count()
                                == 1
                        })
                    }
                    _ => None,
                };
                let Some(candidate) = candidate else {
                    return Err(invalid(
                        &self.root,
                        format!(
                            "ambiguous instance include for {:08x}:{:08x}",
                            id.type_id, id.instance_id
                        ),
                    ));
                };
                let candidate = candidate.clone();
                if self.reference_mode != ReferenceMode::Map && self.external_ids.contains(&id) {
                    return Err(invalid(
                        &candidate.path,
                        "instance and external reference resolve to the same asset ID",
                    ));
                }
                let document = Document::parse(&candidate.xml)
                    .map_err(|error| invalid(&candidate.path, error))?;
                let node = document
                    .get_node(candidate.node)
                    .ok_or_else(|| invalid(&candidate.path, "missing instance declaration"))?;
                let asset = self.compile_static_node(node, &candidate.path, &candidate.source)?;
                self.ids.insert(asset.id);
                self.assets.push(asset);
            }
            index += 1;
        }
        Ok(())
    }

    fn compile_static_node(
        &mut self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let cleaned = if node.attribute("inheritFrom").is_none() {
            self.schema_cleaned_xml(node, path)?
        } else {
            None
        };
        let document = reparse_cleaned(cleaned.as_deref(), path)?;
        let node = cleaned_node(node, document.as_ref());
        match node.tag_name().name() {
            "AudioFile" => self.compile_audio_file(node, path, source),
            "AudioFileMP3Passthrough" => self.compile_mp3_passthrough(node, path, source),
            "OnDemandTexture" | "Texture" => self.compile_texture(node, path, source),
            "PathMusicEvent" | "PathMusicMap" | "PathMusicTrack" => {
                self.compile_music(node, path, source)
            }
            "AIPersonalityDefinition" | "AIMicroManagerData"
                if node.attribute("inheritFrom").is_some() =>
            {
                self.compile_inherited_ai(node, path, source)
            }
            "GameObject"
            | "AmbientStream"
            | "AudioEvent"
            | "AudioEventOverridable"
            | "LogicCommandSet"
            | "LogicCommand"
            | "ObjectFilterAsset"
            | "PlayerTech"
            | "PlayerTemplate"
            | "AttributeModifier"
            | "UpgradeTemplate"
            | "UnitTypeIcon"
            | "UIDecalDestinationMarker"
            | "DynamicsSettings"
            | "SpecialPowerTemplate"
            | "WeaponTemplate"
            | "ArmorTemplate"
            | "ExperienceLevelTemplate"
            | "ObjectCreationList"
            | "Language"
            | "ProjectilePath"
            | "RegisteredObjectSet"
            | "MeshInstanceTemplate"
            | "RedAlertTemplate"
            | "ShadowMap"
            | "UnitOverlayIconSettings"
            | "Weather"
            | "FontDefaultSetting"
            | "FontSubstitution"
            | "FormationAssistant"
            | "FormationAssistantTemplate"
            | "FormationAssistantUnitDefinition"
            | "FormationSelectionTemplate"
            | "ImageSequence"
            | "DebugHotKeys"
            | "AIStateDefinition"
            | "ForceFeedbackEvent"
            | "EvaEvent"
            | "LocomotorTemplate"
            | "BounceKickTerrainMapping"
            | "ConnectionLineManager"
            | "PathMusicGameDynamicState"
            | "TerrainParticleMapping"
            | "FXList"
                if node.attribute("inheritFrom").is_some() =>
            {
                self.compile_schema_inherited_asset(node, path, source)
            }
            "TerrainTextureAtlas" => self.compile_terrain_atlas(node, path, source),
            _ => self.compile_schema_leaf(node, path, source),
        }
    }

    fn schema_cleaned_xml(&self, node: Node<'_, '_>, path: &Path) -> io::Result<Option<String>> {
        if let Some(schema) = &self.game_object_schema
            && self.reference_mode.validates_schema()
        {
            return schema.validate(node, path);
        }
        Ok(None)
    }

    fn compile_effective_asset(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let cleaned = self.schema_cleaned_xml(node, path)?;
        let document = reparse_cleaned(cleaned.as_deref(), path)?;
        self.compile_schema_leaf(cleaned_node(node, document.as_ref()), path, source)
    }

    fn compile_schema_leaf(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let mut asset = static_assets::compile_asset(node, path, source)?;
        if let Some(schema) = &self.game_object_schema {
            if self.reference_mode == ReferenceMode::Map {
                asset.weak_references = schema.weak_references(node, path, true)?;
            } else if self.reference_mode == ReferenceMode::SdkStatic && warnings_enabled() {
                asset.weak_references = schema
                    .weak_references(node, path, false)
                    .unwrap_or_default();
            }
        }
        Ok(asset)
    }

    fn resolve(&self, document: &Path, value: &str) -> io::Result<PathBuf> {
        let value = value.trim().replace('\\', "/");
        let alias = value.split_once(':');
        let relative = alias.map_or(value.as_str(), |(_, relative)| relative);
        let path = Path::new(relative);
        if relative.is_empty() || relative.contains(':') || path.has_root() {
            return Err(invalid(
                document,
                format!("expected relative path {value:?}"),
            ));
        }
        let path = if let Some((alias, _)) = alias {
            let alias = alias.to_ascii_lowercase();
            let roots = match alias.as_str() {
                "data" => &self.data_paths[..],
                "art" => &self.art_paths,
                "audio" => &self.audio_paths,
                "root" => std::slice::from_ref(&self.root),
                _ => {
                    return Err(invalid(
                        document,
                        format!("unknown path alias in {value:?}"),
                    ));
                }
            };
            let bucket = if alias == "art" && !relative.contains('/') {
                relative
                    .get(..2)
                    .ok_or_else(|| invalid(document, "ART file name has no bucket"))?
            } else {
                ""
            };
            roots
                .iter()
                .find_map(|root| {
                    let candidate = root.join(bucket).join(path);
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                    if self.map_art_path.as_ref() == Some(root) && !bucket.is_empty() {
                        let flat = root.join(path);
                        return flat.is_file().then_some(flat);
                    }
                    None
                })
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "{} ({value:?} not found on the SDK {alias} path)",
                            document.display()
                        ),
                    )
                })?
        } else {
            document.with_file_name(path)
        };
        let resolved = read_with_context(&path, fs::canonicalize)?;
        if !resolved.starts_with(&self.root)
            && !self
                .redirected_apt
                .as_ref()
                .is_some_and(|apt| resolved.starts_with(&apt.physical))
        {
            return Err(invalid(&resolved, "path lies outside the data root"));
        }
        Ok(resolved)
    }

    fn read_reference(&mut self, document: &Path, source: &str) -> io::Result<()> {
        let target = self.resolve(document, source).map_err(|error| {
            if self.reference_mode == ReferenceMode::Map {
                self.map_include_error(document, source, error)
            } else {
                error
            }
        })?;
        let relative = target
            .strip_prefix(&self.root)
            .map_err(|_| invalid(document, "reference lies outside the data root"))?
            .with_extension("manifest");
        let mut name = relative
            .to_str()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| invalid(document, "manifest reference path must be ASCII"))?
            .replace('/', "\\")
            .to_ascii_lowercase();
        let directory = self.reference_directory.unwrap();
        let mut relative = relative;
        if let Some(file @ ("static.manifest" | "global.manifest" | "audio.manifest")) =
            name.strip_prefix("sagexml\\")
        {
            if self.reference_mode == ReferenceMode::Map {
                name = file.to_owned();
            } else if self.reference_mode == ReferenceMode::SdkStatic
                && !directory.join(&relative).is_file()
            {
                // The SDK source may live under SageXml while bootstrapped catalogs live at the root.
                relative = PathBuf::from(file);
                name = file.to_owned();
            }
        }
        let path = if self.reference_mode == ReferenceMode::SdkStatic {
            sdk_manifest_path(directory, &relative, &name)
        } else if self.reference_mode == ReferenceMode::Map
            && SDK_BASE_MANIFESTS.contains(&name.as_str())
        {
            let found = directory.join(&relative);
            if found.is_file() {
                found
            } else {
                sdk_manifest_path(directory, Path::new(&name), &name)
            }
        } else {
            directory.join(relative)
        };
        if let Some(references) = self.source_references.as_deref_mut() {
            let local = references.is_local(&target);
            let exists = path.try_exists()?;
            if local || !exists {
                let stem = references.claim(&target)?;
                let manifest_name = format!("{}.manifest", stem.display());
                name = references
                    .relative_directory
                    .join(&manifest_name)
                    .to_str()
                    .filter(|name| name.is_ascii())
                    .ok_or_else(|| invalid(document, "manifest reference path must be ASCII"))?
                    .replace('/', "\\")
                    .to_ascii_lowercase();
                if !self.referenced_manifests.contains(&name) {
                    let sibling = references.output.join(&manifest_name);
                    let stream_root = references.output.clone();
                    let precompiled = if references.string_hashes.is_some() {
                        None
                    } else if local && exists {
                        Some(&path)
                    } else if local && sibling.try_exists()? {
                        Some(&sibling)
                    } else {
                        None
                    };
                    let reused = match precompiled {
                        Some(path) => {
                            match self
                                .read_reference_manifest_with_dependencies(path, Some(&stream_root))
                            {
                                Ok(()) => true,
                                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                                Err(error) => return Err(error),
                            }
                        }
                        None => false,
                    };
                    if !reused {
                        let ids = self
                            .source_references
                            .as_deref_mut()
                            .unwrap()
                            .build(&target, stem, &self.root, directory)?;
                        self.external_ids.extend(ids.iter().copied());
                    }
                    self.referenced_manifests.push(name);
                }
                return Ok(());
            }
        }
        if !self.referenced_manifests.contains(&name) {
            self.read_reference_manifest(&path)?;
            self.referenced_manifests.push(name);
        }
        Ok(())
    }

    fn read_reference_manifest(&mut self, path: &Path) -> io::Result<()> {
        self.read_reference_manifest_with_dependencies(path, None)
    }

    fn read_reference_manifest_with_dependencies(
        &mut self,
        path: &Path,
        stream_root: Option<&Path>,
    ) -> io::Result<()> {
        let bytes = read_with_context(path, fs::read)?;
        let manifest = Manifest::parse(&bytes).map_err(|error| invalid(path, error))?;
        if manifest.version != 6
            || manifest.byte_order != ByteOrder::Little
            || (manifest.all_types_hash != RA3_ALL_TYPES_HASH
                && !is_stock_audio_catalog(path, &manifest))
        {
            return Err(invalid(path, "reference must be an RA3 PC manifest"));
        }
        if let Some(root) = stream_root {
            self.external_ids.extend(crate::fix_assets::external_ids(
                &manifest,
                path.parent().unwrap(),
                root,
                self.reference_directory.unwrap(),
            )?);
        }
        self.external_ids
            .extend(manifest.assets.iter().map(|asset| asset.id));
        if path
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(AUDIO_BASE))
            && is_stock_audio_catalog(Path::new(AUDIO_BASE), &manifest)
        {
            let patch = path.with_file_name(AUDIO_PATCH);
            if patch.is_file() {
                self.read_reference_manifest(&patch)?;
            }
        }
        Ok(())
    }

    fn compile_apt_asset(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let type_name = node.tag_name().name();
        let type_hash = match (node.tag_name().namespace(), type_name) {
            (Some(SAGE_NS), "AptAptData") => 0xe238_fac3,
            (Some(SAGE_NS), "AptConstData") => 0xe70a_e718,
            (Some(SAGE_NS), "AptDatData") => 0x0db6_1c30,
            (Some(SAGE_NS), "AptGeometryData") => 0x52dd_4bea,
            (Some(SAGE_NS), "Texture") => 0x9f5f_f8da,
            _ => return Err(invalid(path, format!("unsupported asset type {type_name}"))),
        };
        let geometry = type_name == "AptGeometryData";
        let texture = type_name == "Texture";
        let attribute_count = if texture {
            if node.attribute("OutputFormat") != Some("A8R8G8B8")
                || node.attribute("GenerateMipMaps") != Some("false")
                || node.attribute("AllowAutomaticResize") != Some("false")
                || !matches!(node.attribute("Type"), None | Some("StandardTexture"))
            {
                return Err(invalid(path, "unsupported AptUI texture settings"));
            }
            5 + usize::from(node.attribute("Type").is_some())
        } else {
            2 + usize::from(geometry)
        };
        if node.attributes().len() != attribute_count || has_content(node) {
            return Err(invalid(path, format!("unexpected content in {type_name}")));
        }
        let id = required(node, "id", path)?;
        if id.contains(':') {
            return Err(invalid(path, format!("invalid asset ID {id:?}")));
        }
        let file = required(node, "File", path)?;
        let geometry_id = if geometry {
            Some(
                required(node, "AptID", path)?
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, "AptID must be a signed 32-bit integer"))?,
            )
        } else {
            None
        };
        let asset_id = AssetId {
            type_id: fast_hash(type_name.as_bytes()),
            instance_id: fast_hash(id.to_ascii_lowercase().as_bytes()),
        };
        let name = format!("{type_name}:{id}");

        let input = self.resolve(path, file)?;
        let bytes = read_with_context(&input, fs::read)?;
        let processing_hash = if texture { 0x9f5f_e8b0 } else { type_hash ^ 4 };
        let instance_hash = hash_asset(node, processing_hash, &bytes, path)?;
        let bytes = if texture {
            texture::tga_to_dds(&bytes).map_err(|error| invalid(&input, error))?
        } else {
            bytes
        };
        let length = u32::try_from(bytes.len()).map_err(|_| invalid(path, "blob is too large"))?;
        let header_size = if geometry { 16u32 } else { 12u32 };
        let instance_size = length
            .checked_next_multiple_of(4)
            .and_then(|size| size.checked_add(header_size))
            .filter(|&size| size <= i32::MAX as u32)
            .ok_or_else(|| invalid(path, "blob is too large"))?;
        let mut payload = Vec::with_capacity(instance_size as usize);
        payload.extend(0u32.to_le_bytes());
        payload.extend(header_size.to_le_bytes());
        payload.extend(length.to_le_bytes());
        if let Some(id) = geometry_id {
            payload.extend(id.to_le_bytes());
        }
        payload.extend(bytes);
        payload.resize(instance_size as usize, 0);
        Ok(CompiledAsset {
            id: asset_id,
            type_hash,
            instance_hash,
            name,
            source: source.to_owned(),
            payload,
            relocations: RELOCATIONS.to_vec(),
            imports: Vec::new(),
            references: Vec::new(),
            weak_references: Vec::new(),
            custom_data: None,
            tokenized: false,
            sort_order: 0,
        })
    }

    fn warn_missing_weak_references(&self) {
        let unresolved = self.assets.iter().flat_map(|asset| {
            asset.weak_references.iter().filter_map(move |reference| {
                (reference.local_id(&self.ids).is_none()
                    && reference.external_id(&self.external_ids).is_none())
                .then_some((asset, reference))
            })
        });
        let mut unresolved = unresolved.peekable();
        if unresolved.peek().is_none() {
            return;
        }

        let directory = self.reference_directory.unwrap();
        let mut stock = HashSet::new();
        for name in [
            AUDIO_BASE,
            AUDIO_PATCH,
            "global.manifest",
            "static.manifest",
            "sagexml/worldbuilder.manifest",
            "worldbuilder.manifest",
        ] {
            // These catalogs supplement the declared references for diagnostics only.
            let path = directory.join(name);
            if let Ok(bytes) = fs::read(&path)
                && let Ok(manifest) = Manifest::parse(&bytes)
                && manifest.version == 6
                && manifest.byte_order == ByteOrder::Little
                && (manifest.all_types_hash == RA3_ALL_TYPES_HASH
                    || is_stock_audio_catalog(&path, &manifest))
            {
                stock.extend(manifest.assets.iter().map(|asset| asset.id));
            }
        }
        let mut reported = HashSet::new();
        for (asset, reference) in unresolved {
            if reference.external_id(&stock).is_none() && reported.insert((asset.id, reference.id))
            {
                warning(format_args!(
                    "Unknown asset '{}' referenced from '{}' in '{}'",
                    reference.diagnostic_name(),
                    asset.name,
                    asset.source
                ));
            }
        }
    }

    fn finish(mut self) -> io::Result<(CompiledStream, HashSet<AssetId>)> {
        if self.assets.is_empty()
            && (self.reference_directory.is_none() || self.reference_mode == ReferenceMode::Map)
        {
            return Err(invalid(&self.root, "input contains no supported assets"));
        }
        if self.reference_mode == ReferenceMode::Implicit
            && self
                .assets
                .iter()
                .flat_map(|asset| &asset.references)
                .any(|reference| {
                    reference.local_id(&self.ids).is_none()
                        && reference.external_id(&self.external_ids).is_none()
                })
        {
            let root = self.reference_directory.unwrap();
            for file in SDK_BASE_MANIFESTS {
                let path = sdk_manifest_path(root, Path::new(file), file);
                if path.is_file() {
                    self.read_reference_manifest(&path)?;
                }
            }
        }
        self.assets
            .sort_by_cached_key(|asset| (asset.sort_order, asset_name_order(&asset.name)));
        if self.reference_mode == ReferenceMode::SdkStatic {
            self.warn_missing_weak_references();
        }
        let mut files = LinkedFiles {
            bin: vec![0; 4],
            relo: vec![0; 4],
            imp: vec![0; 4],
        };
        let mut checksums = Vec::new();
        let mut max_chunk_sizes = ChunkSizes {
            instance: 0,
            relocations: 0,
            imports: 0,
        };
        let worldbuilder_ids: HashSet<_> = self.worldbuilder_hashes.keys().copied().collect();
        let mut resolved_references = Vec::with_capacity(self.assets.len());
        let mut deferred = HashSet::new();
        for asset in &self.assets {
            let mut resolved = Vec::with_capacity(asset.references.len());
            for reference in &asset.references {
                if let Some(local) = reference.local_id(&self.ids) {
                    resolved.push(local);
                    continue;
                }
                let Some(external) = reference
                    .external_id(&self.external_ids)
                    .or_else(|| reference.external_id(&worldbuilder_ids))
                else {
                    if self.reference_mode == ReferenceMode::SdkStatic {
                        let id = reference.declared_id();
                        resolved.push(id);
                        if deferred.insert(id) {
                            warning(format_args!(
                                "Unknown asset '{}' referenced from '{}' in '{}'",
                                reference.diagnostic_name(),
                                asset.name,
                                asset.source
                            ));
                        }
                        continue;
                    }
                    // Map placements can reference assets supplied by the game's active streams.
                    if self.reference_mode == ReferenceMode::Map
                        && asset.name.starts_with("GameMap:")
                    {
                        let id = reference.declared_id();
                        resolved.push(id);
                        if deferred.insert(id) {
                            eprintln!(
                                "Map runtime reference {:08x}:{:08x} in {}",
                                id.type_id, id.instance_id, asset.name
                            );
                        }
                        continue;
                    }
                    let mut message = format!(
                        "unresolved reference {} in {} ({})",
                        reference.diagnostic_name(),
                        asset.name,
                        asset.source
                    );
                    if self.reference_mode == ReferenceMode::Implicit {
                        let directory = self.reference_directory.unwrap();
                        message.push_str(&format!(
                            ". Check that the base SDK manifests are present and current in '{}' or '{}'",
                            directory.display(),
                            directory.join("sagexml").display()
                        ));
                    }
                    return Err(invalid(&self.root, message));
                };
                resolved.push(external);
            }
            resolved_references.push(resolved);
        }
        let order = if self.reference_mode == ReferenceMode::SdkStatic {
            stream_order::stable_order(
                &self.assets,
                &resolved_references,
                self.game_object_schema.as_ref(),
            )
        } else {
            (0..self.assets.len()).collect()
        };
        let assets = order
            .into_iter()
            .map(|index| {
                let asset = &self.assets[index];
                let references = &resolved_references[index];
                files.bin.extend_from_slice(&asset.payload);
                files.relo.extend_from_slice(&asset.relocations);
                files.imp.extend_from_slice(&asset.imports);
                for word in [
                    asset.id.type_id,
                    asset.type_hash,
                    asset.id.instance_id,
                    asset.instance_hash,
                    asset.references.len() as u32,
                ] {
                    checksums.extend(word.to_le_bytes());
                }
                let chunk_sizes = ChunkSizes {
                    instance: asset.payload.len() as u32,
                    relocations: asset.relocations.len() as u32,
                    imports: asset.imports.len() as u32,
                };
                max_chunk_sizes.instance = max_chunk_sizes.instance.max(chunk_sizes.instance);
                max_chunk_sizes.relocations =
                    max_chunk_sizes.relocations.max(chunk_sizes.relocations);
                max_chunk_sizes.imports = max_chunk_sizes.imports.max(chunk_sizes.imports);
                AssetEntry {
                    id: asset.id,
                    type_hash: asset.type_hash,
                    instance_hash: self
                        .worldbuilder_hashes
                        .get(&asset.id)
                        .copied()
                        .unwrap_or(asset.instance_hash),
                    chunk_sizes,
                    name: asset.name.as_bytes(),
                    source_file_name: asset.source.as_bytes(),
                    references: AssetReferences::from_ids(references),
                    tokenized: Some(u32::from(asset.tokenized)),
                }
            })
            .collect();
        let stream_checksum = if checksums.is_empty() {
            0
        } else {
            memory_stream_hash(checksums)
        };
        for output in [&mut files.bin, &mut files.relo, &mut files.imp] {
            output[..4].copy_from_slice(&stream_checksum.to_le_bytes());
        }
        let manifest = Manifest {
            version: 6,
            byte_order: ByteOrder::Little,
            is_linked: true,
            stream_checksum,
            all_types_hash: RA3_ALL_TYPES_HASH,
            total_instance_data_size: u32::try_from(files.bin.len() - 4)
                .map_err(|_| invalid(&self.root, "stream is too large"))?,
            max_chunk_sizes,
            assets,
            referenced_manifests: (self.reference_mode == ReferenceMode::Map)
                .then_some(ReferencedManifest {
                    path: b"worldbuilder.11.manifest",
                    is_patch: true,
                })
                .into_iter()
                .chain(
                    self.referenced_manifests
                        .iter()
                        .map(|path| ReferencedManifest {
                            path: path.as_bytes(),
                            is_patch: false,
                        }),
                )
                .collect(),
        }
        .encode()
        .map_err(io::Error::other)?;
        let custom_data = self
            .assets
            .into_iter()
            .filter_map(|asset| {
                let data = asset.custom_data?;
                Some((
                    format!(
                        "{:08x}.{:08x}.{:08x}.{:08x}.cdata",
                        asset.id.type_id,
                        asset.type_hash,
                        asset.id.instance_id,
                        asset.instance_hash
                    ),
                    data,
                ))
            })
            .collect();
        if self.source_references.is_some() {
            self.external_ids.extend(self.ids);
        }
        Ok((
            CompiledStream {
                manifest,
                files,
                custom_data,
            },
            self.external_ids,
        ))
    }
}

fn asset_name_order(name: &str) -> (Vec<u8>, Vec<Reverse<usize>>) {
    // SDK collation places '.' and '_' before digits and compares hyphens after the base ID.
    let mut primary = Vec::with_capacity(name.len());
    let mut hyphens = Vec::new();
    for byte in name.bytes() {
        match byte {
            b'-' => hyphens.push(Reverse(primary.len())),
            b'_' => primary.push(b'.' + 1),
            _ => primary.push(byte.to_ascii_lowercase()),
        }
    }
    (primary, hyphens)
}

fn has_content(node: Node<'_, '_>) -> bool {
    node.children().any(|child| {
        child.is_element()
            || child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
    })
}

fn required<'a>(node: Node<'a, '_>, name: &str, path: &Path) -> io::Result<&'a str> {
    node.attribute(name)
        .filter(|value| {
            !value.trim().is_empty()
                && value
                    .bytes()
                    .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        })
        .ok_or_else(|| invalid(path, format!("expected a nonempty ASCII {name} attribute")))
}

fn hash_asset(
    node: Node<'_, '_>,
    processing_hash: u32,
    bytes: &[u8],
    path: &Path,
) -> io::Result<u32> {
    let xml_hash = hash_xml(node, processing_hash, path)?;
    let length =
        u32::try_from(bytes.len()).map_err(|_| invalid(path, "source file is too large"))?;
    let file_hash = bytes
        .chunks(FILE_HASH_BLOCK)
        .fold(length, fast_hash_with_seed);
    Ok(xml_hash ^ memory_stream_hash(file_hash.to_le_bytes().to_vec()))
}

pub(crate) fn hash_source_file(path: &Path, mut copy: impl Write) -> io::Result<u32> {
    let mut file = fs::File::open(path)?;
    let length = u32::try_from(file.metadata()?.len())
        .map_err(|_| invalid(path, "source file is too large"))?;
    let mut remaining = length as usize;
    let mut buffer = vec![0; FILE_HASH_BLOCK.min(remaining)];
    let mut hash = length;
    while remaining != 0 {
        let size = buffer.len().min(remaining);
        let chunk = &mut buffer[..size];
        file.read_exact(chunk)?;
        hash = fast_hash_with_seed(hash, chunk);
        copy.write_all(chunk)?;
        remaining -= size;
    }
    if file.read(&mut [0])? != 0 {
        return Err(invalid(path, "source file changed during compilation"));
    }
    Ok(hash)
}

fn hash_xml(node: Node<'_, '_>, processing_hash: u32, path: &Path) -> io::Result<u32> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"Windows-1252\"?>");
    write_xml(node, true, &mut xml);
    let xml = windows_1252(&xml)
        .ok_or_else(|| invalid(path, "asset XML contains characters outside Windows-1252"))?;
    let seed = fast_hash_with_seed(processing_hash, b"11");
    // The legacy HashingWriter omits its last block when the length is an exact multiple.
    let hashed_len = if xml.len().is_multiple_of(512) {
        xml.len() - 512
    } else {
        xml.len()
    };
    Ok(xml[..hashed_len]
        .chunks(512)
        .fold(seed, fast_hash_with_seed))
}

fn windows_1252(text: &str) -> Option<Cow<'_, [u8]>> {
    const EXTENDED: &str = "€\u{81}‚ƒ„…†‡ˆ‰Š‹Œ\u{8d}Ž\u{8f}\u{90}‘’“”•–—˜™š›œ\u{9d}žŸ";
    if text.is_ascii() {
        return Some(Cow::Borrowed(text.as_bytes()));
    }
    text.chars()
        .map(|character| match character as u32 {
            0..=0x7f | 0xa0..=0xff => Some(character as u8),
            _ => EXTENDED
                .chars()
                .position(|extended| extended == character)
                .map(|index| 0x80 + index as u8),
        })
        .collect::<Option<Vec<_>>>()
        .map(Cow::Owned)
}

fn write_xml(node: Node<'_, '_>, root: bool, xml: &mut String) {
    let input = node.document().input_text();
    let range = node.range();
    let original = &input[range.clone()];
    let name = original[1..]
        .split(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
        .next()
        .expect("parsed XML element name");
    let prefix = name.split_once(':').map(|(prefix, _)| prefix);
    let preserve_namespaces = node.attribute((XML_NS, "base")).is_some();
    let mut attribute_end = range.start + 1 + name.len();
    let mut has_namespace = false;
    xml.push('<');
    xml.push_str(name);
    for attr in node.attributes() {
        if preserve_namespaces {
            has_namespace |= write_namespace_attributes(
                node,
                prefix,
                &input[attribute_end..attr.range().start],
                xml,
            );
            attribute_end = attr.range().end;
        }
        xml.push(' ');
        xml.push_str(&input[attr.range_qname()]);
        xml.push_str("=\"");
        escape_xml(attr.value(), true, xml);
        xml.push('"');
    }
    if preserve_namespaces {
        has_namespace |= write_namespace_attributes(
            node,
            prefix,
            &input[attribute_end..range.start + opening_tag_end(original)],
            xml,
        );
    }
    if root && !has_namespace {
        xml.push_str(" xmlns");
        if let Some(prefix) = prefix {
            xml.push(':');
            xml.push_str(prefix);
        }
        xml.push_str("=\"uri:ea.com:eala:asset\"");
    }
    if original.ends_with("/>") {
        xml.push_str(" />");
        return;
    }
    xml.push('>');
    for child in node.children() {
        match child.node_type() {
            roxmltree::NodeType::Element => write_xml(child, false, xml),
            roxmltree::NodeType::Text => {
                let text = child.text().unwrap();
                if !is_xml_whitespace(text) {
                    // The Windows XmlWriter normalizes line breaks in text to CRLF.
                    for line in text.split_inclusive('\n') {
                        if let Some(line) = line.strip_suffix('\n') {
                            escape_xml(line, false, xml);
                            xml.push_str("\r\n");
                        } else {
                            escape_xml(line, false, xml);
                        }
                    }
                }
            }
            roxmltree::NodeType::Comment => {
                xml.push_str("<!--");
                xml.push_str(child.text().unwrap());
                xml.push_str("-->");
            }
            _ => {}
        }
    }
    xml.push_str("</");
    xml.push_str(name);
    xml.push('>');
}

fn opening_tag_end(source: &str) -> usize {
    let mut quote = None;
    for (index, byte) in source.bytes().enumerate() {
        match quote {
            Some(end) if end == byte => quote = None,
            Some(_) => {}
            None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
            None if byte == b'>' => return index,
            None => {}
        }
    }
    unreachable!("parsed XML element has an opening tag")
}

fn write_namespace_attributes(
    node: Node<'_, '_>,
    element_prefix: Option<&str>,
    mut raw: &str,
    xml: &mut String,
) -> bool {
    let mut has_namespace = false;
    while let Some((name, value)) = raw.trim_start().split_once('=') {
        let name = name.trim_end();
        let prefix = name.strip_prefix("xmlns:");
        let value = value.trim_start();
        let delimiter = value.as_bytes()[0] as char;
        let (_, rest) = value[1..]
            .split_once(delimiter)
            .expect("parsed XML attribute");
        has_namespace |= prefix == element_prefix;
        if let Some(uri) = node.lookup_namespace_uri(prefix) {
            xml.push(' ');
            xml.push_str(name);
            xml.push_str("=\"");
            escape_xml(uri, true, xml);
            xml.push('"');
        }
        raw = rest;
    }
    has_namespace
}

fn is_xml_whitespace(text: &str) -> bool {
    text.bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn escape_xml(text: &str, attribute: bool, xml: &mut String) {
    for character in text.chars() {
        match character {
            '&' => xml.push_str("&amp;"),
            '<' => xml.push_str("&lt;"),
            '>' => xml.push_str("&gt;"),
            '"' if attribute => xml.push_str("&quot;"),
            '\r' if attribute => xml.push_str("&#xD;"),
            '\n' if attribute => xml.push_str("&#xA;"),
            '\t' if attribute => xml.push_str("&#x9;"),
            _ => xml.push(character),
        }
    }
}

fn memory_stream_hash(mut bytes: Vec<u8>) -> u32 {
    // EA hashes the MemoryStream capacity, including the unused zero bytes.
    bytes.resize(bytes.len().next_power_of_two().max(256), 0);
    fast_hash(&bytes)
}

fn reparse_cleaned<'a>(xml: Option<&'a str>, path: &Path) -> io::Result<Option<Document<'a>>> {
    xml.map(Document::parse)
        .transpose()
        .map_err(|error| invalid(path, error))
}

fn cleaned_node<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    document: Option<&'a Document<'input>>,
) -> Node<'a, 'input> {
    document.map_or(node, |document| {
        document
            .descendants()
            .find(|cleaned| cleaned.is_element() && cleaned.range().start == node.range().start)
            .expect("schema validation preserves the asset element")
    })
}

fn warnings_enabled() -> bool {
    std::env::var_os("BAB_NO_WARNINGS").is_none_or(|value| value != "1")
}

pub(crate) fn warning(message: std::fmt::Arguments<'_>) {
    if !warnings_enabled() {
        return;
    }
    let line = format!("Warning: {message}\n");
    STREAM_WARNINGS.with_borrow_mut(|warnings| {
        if let Some(warnings) = warnings {
            warnings.extend_from_slice(line.as_bytes());
        } else {
            write_warnings(line.as_bytes());
        }
    });
}

thread_local! {
    static STREAM_WARNINGS: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

// Used on a dedicated stream worker that terminates when compilation finishes.
pub(crate) fn capture_stream_warnings<T>(compile: impl FnOnce() -> T) -> (T, Vec<u8>) {
    STREAM_WARNINGS.set(Some(Vec::new()));
    let result = compile();
    (result, STREAM_WARNINGS.take().unwrap())
}

pub(crate) fn write_warnings(warnings: &[u8]) {
    // Mod Studio combines both output streams and can interleave partial writes.
    let _ = io::stderr().lock().write_all(warnings);
}

fn invalid(path: &Path, error: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} ({error})", path.display()),
    )
}

fn read_with_context<'a, T>(
    path: &'a Path,
    read: impl FnOnce(&'a Path) -> io::Result<T>,
) -> io::Result<T> {
    read(path)
        .map_err(|error| io::Error::new(error.kind(), format!("{} ({error})", path.display())))
}
