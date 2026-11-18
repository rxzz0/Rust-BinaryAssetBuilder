//! Compiles supported Red Alert 3 PC XML streams.

use crate::hash::{fast_hash, fast_hash_with_seed};
use crate::linked_stream::LinkedFiles;
use crate::manifest::{
    AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest, ReferencedManifest,
};
use expressions::{Definitions, expand_assets, merge_definitions, read_definitions};
use roxmltree::{Document, Node, NodeId};
use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

mod coverage;
mod expressions;
mod merge_assets;
mod precompiled;
mod sdk_static;
mod static_assets;
mod texture;
mod xinclude;

pub use crate::compile_batch::{compile_apt_ui_directory, write_compiled_stream};
pub use coverage::{sdk_coverage, verify_ea_batch};
pub use merge_assets::merge_assets;
pub use sdk_static::{compile_sdk_global, compile_sdk_static};

const SAGE_NS: &str = "uri:ea.com:eala:asset";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const FILE_HASH_BLOCK: usize = 0x100000;
const RA3_ALL_TYPES_HASH: u32 = 0x54ee_e764;
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
    compile(input, data_root, None, false)
}

/// Compiles supported static XML assets against the SDK's precompiled streams.
pub fn compile_static(
    input: &Path,
    data_root: &Path,
    reference_directory: &Path,
) -> io::Result<CompiledStream> {
    compile(input, data_root, Some(reference_directory), false)
}

fn compile(
    input: &Path,
    data_root: &Path,
    reference_directory: Option<&Path>,
    implicit_sdk_references: bool,
) -> io::Result<CompiledStream> {
    let root = fs::canonicalize(data_root)?;
    let input = fs::canonicalize(input)?;
    let mod_root = input.strip_prefix(&root).ok().and_then(|relative| {
        let mut components = relative.iter();
        let mods = components.next()?;
        let name = components.next()?;
        mods.eq_ignore_ascii_case("mods")
            .then(|| root.join(mods).join(name))
    });
    let mut compiler = Compiler {
        data_paths: search_paths(&root, mod_root.as_deref(), "Data", "SageXml"),
        art_paths: search_paths(&root, mod_root.as_deref(), "Art", "Art"),
        audio_paths: search_paths(&root, mod_root.as_deref(), "Audio", "Audio"),
        root,
        visited: HashSet::new(),
        active: HashSet::new(),
        ids: HashSet::new(),
        static_asset_locations: HashMap::new(),
        next_document_order: 0,
        assets: Vec::new(),
        tentative: HashMap::new(),
        definitions: HashMap::new(),
        xml_fragments: HashMap::new(),
        game_object_schema: None,
        music_headers: HashMap::new(),
        reference_directory,
        implicit_sdk_references,
        referenced_manifests: Vec::new(),
        external_ids: HashSet::new(),
    };
    compiler.read_document(&input, false, false)?;
    compiler.add_referenced_instances()?;
    compiler.finish()
}

fn search_paths(
    root: &Path,
    mod_root: Option<&Path>,
    directory: &str,
    sdk_directory: &str,
) -> Vec<PathBuf> {
    let mut paths = vec![root.to_path_buf()];
    if let Some(mod_root) = mod_root {
        paths.push(mod_root.join(directory));
    }
    paths.extend([root.join("Mods"), root.join(sdk_directory)]);
    paths
}

struct Compiler<'a> {
    root: PathBuf,
    data_paths: Vec<PathBuf>,
    art_paths: Vec<PathBuf>,
    audio_paths: Vec<PathBuf>,
    visited: HashSet<(PathBuf, bool)>,
    active: HashSet<PathBuf>,
    ids: HashSet<AssetId>,
    static_asset_locations: HashMap<AssetId, (usize, usize)>,
    next_document_order: usize,
    assets: Vec<CompiledAsset>,
    tentative: HashMap<AssetId, Vec<TentativeAsset>>,
    definitions: HashMap<PathBuf, Rc<Definitions>>,
    xml_fragments: HashMap<PathBuf, Rc<str>>,
    game_object_schema: Option<static_assets::game_object::Schema>,
    music_headers: HashMap<PathBuf, static_assets::music::Header>,
    reference_directory: Option<&'a Path>,
    implicit_sdk_references: bool,
    referenced_manifests: Vec<String>,
    external_ids: HashSet<AssetId>,
}

#[derive(Clone)]
struct TentativeAsset {
    path: PathBuf,
    source: Rc<str>,
    xml: Rc<str>,
    node: NodeId,
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
    custom_data: Option<CustomData>,
    tokenized: bool,
    sort_order: u8,
}

#[derive(PartialEq, Eq)]
struct AssetReference {
    id: AssetId,
    external_type: Option<u32>,
}

impl From<AssetId> for AssetReference {
    fn from(id: AssetId) -> Self {
        Self {
            id,
            external_type: None,
        }
    }
}

const AUDIO_ASSET_TYPE: u32 = 0x4053_c714;
const RENDER_ASSET_TYPE: u32 = 0xb22f_87c9;

impl AssetReference {
    fn candidate_ids(&self) -> impl Iterator<Item = AssetId> + '_ {
        let subtypes: &[&str] = match (self.id.type_id, self.external_type) {
            (RENDER_ASSET_TYPE, _) => &["W3DContainer", "W3DMesh", "W3DCollisionBox"],
            (AUDIO_ASSET_TYPE, _) | (_, Some(AUDIO_ASSET_TYPE)) => &[
                "AudioEvent",
                "Multisound",
                "AmbientStream",
                "DialogEvent",
                "MusicTrack",
            ],
            _ => &[],
        };
        std::iter::once(self.id).chain(subtypes.iter().map(|name| AssetId {
            type_id: fast_hash(name.as_bytes()),
            instance_id: self.id.instance_id,
        }))
    }

    fn local_id(&self, ids: &HashSet<AssetId>) -> Option<AssetId> {
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
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid(path, "XML lies outside the data root"))?;
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
        let mut local_assets = HashSet::new();
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
            } else if tentative {
                let (_, id) = static_assets::declaration_id(node, path)?;
                self.tentative.entry(id).or_default().push(TentativeAsset {
                    path: path.to_path_buf(),
                    source: Rc::clone(&source),
                    xml: Rc::clone(xml),
                    node: node.id(),
                });
            } else {
                let asset = if self.reference_directory.is_some() {
                    self.compile_static_node(node, path, &source)?
                } else {
                    self.compile_apt_asset(node, path, &source)?
                };
                if self.reference_directory.is_some() {
                    if !local_assets.insert(asset.id) {
                        return Err(invalid(path, format!("duplicate asset ID {}", asset.name)));
                    }
                    if let Some((index, previous_order)) =
                        self.static_asset_locations.get_mut(&asset.id)
                    {
                        // An including document takes precedence over its descendants.
                        // Across sibling include trees, EA keeps the first declaration.
                        if document_order < *previous_order {
                            self.assets[*index] = asset;
                            *previous_order = document_order;
                        }
                        continue;
                    }
                    self.static_asset_locations
                        .insert(asset.id, (self.assets.len(), document_order));
                }
                if !self.ids.insert(asset.id) {
                    return Err(invalid(path, format!("duplicate asset ID {}", asset.name)));
                }
                self.assets.push(asset);
            }
        }
        self.active.remove(path);
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
            let target = self.resolve(path, source)?;
            let art_include = source
                .split_once(':')
                .is_some_and(|(alias, _)| alias.eq_ignore_ascii_case("art"));
            self.read_document(&target, included_tentative, art_include)?;
            if let Some(included) = self.definitions.get(&target) {
                merge_definitions(definitions, included, path)?;
            }
        }
        Ok(())
    }

    fn add_referenced_instances(&mut self) -> io::Result<()> {
        if self.tentative.is_empty() {
            return Ok(());
        }
        let mut index = 0;
        let mut pending = Vec::new();
        while index < self.assets.len() {
            pending.extend(
                self.assets[index]
                    .references
                    .iter()
                    .filter(|reference| reference.local_id(&self.ids).is_none())
                    .filter_map(|reference| {
                        reference
                            .candidate_ids()
                            .find(|id| self.tentative.contains_key(id))
                    }),
            );
            for id in pending.drain(..) {
                if self.ids.contains(&id) {
                    continue;
                }
                let Some(candidates) = self.tentative.get(&id) else {
                    continue;
                };
                let [candidate] = candidates.as_slice() else {
                    return Err(invalid(
                        &self.root,
                        format!(
                            "ambiguous instance include for {:08x}:{:08x}",
                            id.type_id, id.instance_id
                        ),
                    ));
                };
                let candidate = candidate.clone();
                if self.external_ids.contains(&id) {
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
        match node.tag_name().name() {
            "AudioFile" => self.compile_audio_file(node, path, source),
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
            | "LogicCommandSet"
            | "LogicCommand"
            | "ObjectFilterAsset"
            | "PlayerTech"
            | "AttributeModifier"
            | "UpgradeTemplate"
            | "UnitTypeIcon"
            | "SpecialPowerTemplate"
            | "WeaponTemplate"
            | "ExperienceLevelTemplate"
            | "EvaEvent"
            | "LocomotorTemplate"
            | "BounceKickTerrainMapping"
            | "TerrainParticleMapping"
            | "FXList"
                if node.attribute("inheritFrom").is_some() =>
            {
                self.compile_schema_inherited_asset(node, path, source)
            }
            _ => static_assets::compile_asset(node, path, source),
        }
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
                .map(|root| root.join(bucket).join(path))
                .find(|candidate| candidate.is_file())
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
        if !resolved.starts_with(&self.root) {
            return Err(invalid(&resolved, "path lies outside the data root"));
        }
        Ok(resolved)
    }

    fn read_reference(&mut self, document: &Path, source: &str) -> io::Result<()> {
        let target = self.resolve(document, source)?;
        let relative = target
            .strip_prefix(&self.root)
            .map_err(|_| invalid(document, "reference lies outside the data root"))?
            .with_extension("manifest");
        let name = relative
            .to_str()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| invalid(document, "manifest reference path must be ASCII"))?
            .replace('/', "\\")
            .to_ascii_lowercase();
        if self.referenced_manifests.contains(&name) {
            return Ok(());
        }
        let path = self.reference_directory.unwrap().join(relative);
        let bytes = read_with_context(&path, fs::read)?;
        let manifest = Manifest::parse(&bytes).map_err(|error| invalid(&path, error))?;
        if manifest.version != 6
            || manifest.byte_order != ByteOrder::Little
            || manifest.all_types_hash != RA3_ALL_TYPES_HASH
        {
            return Err(invalid(&path, "reference must be an RA3 PC manifest"));
        }
        self.external_ids
            .extend(manifest.assets.iter().map(|asset| asset.id));
        self.referenced_manifests.push(name);
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
            custom_data: None,
            tokenized: false,
            sort_order: 0,
        })
    }

    fn finish(mut self) -> io::Result<CompiledStream> {
        if self.assets.is_empty() {
            return Err(invalid(&self.root, "input contains no supported assets"));
        }
        if self.implicit_sdk_references
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
            for file in ["audio.manifest", "global.manifest", "static.manifest"] {
                let path = root.join(file);
                if path.is_file() {
                    let bytes = read_with_context(&path, fs::read)?;
                    let manifest =
                        Manifest::parse(&bytes).map_err(|error| invalid(&path, error))?;
                    if manifest.version != 6
                        || manifest.byte_order != ByteOrder::Little
                        || manifest.all_types_hash != RA3_ALL_TYPES_HASH
                    {
                        return Err(invalid(&path, "expected an RA3 PC reference stream"));
                    }
                    self.external_ids
                        .extend(manifest.assets.iter().map(|asset| asset.id));
                }
            }
        }
        self.assets
            .sort_by_cached_key(|asset| (asset.sort_order, asset_name_order(&asset.name)));
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
        let mut resolved_references = Vec::with_capacity(self.assets.len());
        for asset in &self.assets {
            let mut resolved = Vec::with_capacity(asset.references.len());
            for reference in &asset.references {
                let id = reference.id;
                if let Some(local) = reference.local_id(&self.ids) {
                    resolved.push(local);
                    continue;
                }
                let Some(external) = reference.external_id(&self.external_ids) else {
                    return Err(invalid(
                        &self.root,
                        format!(
                            "unresolved reference {:08x}:{:08x} in {}",
                            id.type_id, id.instance_id, asset.name
                        ),
                    ));
                };
                resolved.push(external);
            }
            resolved_references.push(resolved);
        }
        let assets = self
            .assets
            .iter()
            .zip(&resolved_references)
            .map(|(asset, references)| {
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
                    instance_hash: asset.instance_hash,
                    chunk_sizes,
                    name: asset.name.as_bytes(),
                    source_file_name: asset.source.as_bytes(),
                    references: AssetReferences::from_ids(references),
                    tokenized: Some(u32::from(asset.tokenized)),
                }
            })
            .collect();
        let stream_checksum = memory_stream_hash(checksums);
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
            referenced_manifests: self
                .referenced_manifests
                .iter()
                .map(|path| ReferencedManifest {
                    path: path.as_bytes(),
                    is_patch: false,
                })
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
        Ok(CompiledStream {
            manifest,
            files,
            custom_data,
        })
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
