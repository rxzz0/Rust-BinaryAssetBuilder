//! Compiles supported Red Alert 3 PC XML streams.

use crate::hash::{fast_hash, fast_hash_with_seed};
use crate::linked_stream::LinkedFiles;
use crate::manifest::{
    AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest, ReferencedManifest,
};
use roxmltree::{Document, Node};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

mod static_assets;
mod texture;

const SAGE_NS: &str = "uri:ea.com:eala:asset";
const FILE_HASH_BLOCK: usize = 0x100000;
const RA3_ALL_TYPES_HASH: u32 = 0x54ee_e764;
const RELOCATIONS: [u8; 8] = [4, 0, 0, 0, 255, 255, 255, 255];

pub struct CompiledStream {
    pub manifest: Vec<u8>,
    pub files: LinkedFiles,
}

/// Compiles a linked RA3 PC stream from AptUI declarations and their includes.
pub fn compile_apt_ui(input: &Path, data_root: &Path) -> io::Result<CompiledStream> {
    compile(input, data_root, None)
}

/// Compiles supported static XML assets against the SDK's precompiled streams.
pub fn compile_static(
    input: &Path,
    data_root: &Path,
    reference_directory: &Path,
) -> io::Result<CompiledStream> {
    compile(input, data_root, Some(reference_directory))
}

fn compile(
    input: &Path,
    data_root: &Path,
    reference_directory: Option<&Path>,
) -> io::Result<CompiledStream> {
    let root = fs::canonicalize(data_root)?;
    let input = fs::canonicalize(input)?;
    let mut data_paths = vec![root.clone()];
    if let Ok(relative) = input.strip_prefix(&root) {
        let mut components = relative.iter();
        if let (Some(mods), Some(name)) = (components.next(), components.next())
            && mods.eq_ignore_ascii_case("mods")
        {
            data_paths.push(root.join(mods).join(name).join("Data"));
        }
    }
    data_paths.extend([root.join("Mods"), root.join("SageXml")]);
    let mut compiler = Compiler {
        root,
        data_paths,
        visited: HashSet::new(),
        active: HashSet::new(),
        ids: HashSet::new(),
        assets: Vec::new(),
        reference_directory,
        referenced_manifests: Vec::new(),
        external_ids: HashSet::new(),
    };
    compiler.read_document(&input)?;
    compiler.finish()
}

struct Compiler<'a> {
    root: PathBuf,
    data_paths: Vec<PathBuf>,
    visited: HashSet<PathBuf>,
    active: HashSet<PathBuf>,
    ids: HashSet<AssetId>,
    assets: Vec<CompiledAsset>,
    reference_directory: Option<&'a Path>,
    referenced_manifests: Vec<String>,
    external_ids: HashSet<AssetId>,
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
    references: Vec<AssetId>,
}

impl Compiler<'_> {
    fn read_document(&mut self, path: &Path) -> io::Result<()> {
        if self.active.contains(path) {
            return Err(invalid(path, "include cycle"));
        }
        if self.active.len() >= 128 {
            return Err(invalid(path, "includes are nested more than 128 levels"));
        }
        if !self.visited.insert(path.to_path_buf()) {
            return Ok(());
        }
        self.active.insert(path.to_path_buf());
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| invalid(path, "XML lies outside the data root"))?;
        let relative = relative
            .to_str()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| invalid(path, "source path must be ASCII"))?;
        let source = format!("DATA:{}", relative.replace('\\', "/").to_ascii_lowercase());
        let xml = read_with_context(path, fs::read_to_string)?;
        let document = Document::parse(&xml).map_err(|error| invalid(path, error))?;
        let root = document.root_element();
        if !root.has_tag_name((SAGE_NS, "AssetDeclaration")) || root.attributes().len() != 0 {
            return Err(invalid(path, "expected a SAGE AssetDeclaration root"));
        }
        for node in root.children().filter(Node::is_element) {
            if node.has_tag_name((SAGE_NS, "Includes")) {
                if node.attributes().len() != 0 {
                    return Err(invalid(path, "unsupported Includes attributes"));
                }
                for include in node.children().filter(Node::is_element) {
                    let source = include.attribute("source");
                    if !include.has_tag_name((SAGE_NS, "Include"))
                        || include.attributes().len() != 2
                        || has_content(include)
                    {
                        return Err(invalid(path, "invalid Include element"));
                    }
                    let source = source.ok_or_else(|| invalid(path, "missing include source"))?;
                    match include.attribute("type") {
                        Some("all") => {
                            let target = self.resolve(path, source)?;
                            self.read_document(&target)?;
                        }
                        Some("reference") if self.reference_directory.is_some() => {
                            self.read_reference(path, source)?;
                        }
                        kind => {
                            return Err(invalid(
                                path,
                                format!("unsupported include type {kind:?}"),
                            ));
                        }
                    }
                }
            } else if self.reference_directory.is_some()
                && node.has_tag_name((SAGE_NS, "Tags"))
                && node.attributes().len() == 0
                && !has_content(node)
            {
                continue;
            } else {
                let asset = if self.reference_directory.is_some() {
                    static_assets::compile_asset(node, path, &source)?
                } else {
                    self.compile_apt_asset(node, path, &source)?
                };
                if !self.ids.insert(asset.id) {
                    return Err(invalid(path, format!("duplicate asset ID {}", asset.name)));
                }
                self.assets.push(asset);
            }
        }
        self.active.remove(path);
        Ok(())
    }

    fn resolve(&self, document: &Path, value: &str) -> io::Result<PathBuf> {
        let value = value.trim().replace('\\', "/");
        let data = value
            .split_once(':')
            .filter(|(prefix, _)| prefix.eq_ignore_ascii_case("data"))
            .map(|(_, path)| path);
        let relative = data.unwrap_or(&value);
        let path = Path::new(relative);
        if relative.is_empty() || relative.contains(':') || path.has_root() {
            return Err(invalid(
                document,
                format!("expected relative path {value:?}"),
            ));
        }
        let path = if data.is_some() {
            self.data_paths
                .iter()
                .map(|root| root.join(path))
                .find(|candidate| candidate.is_file())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!(
                            "{} ({value:?} not found on the SDK data path)",
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
        })
    }

    fn finish(mut self) -> io::Result<CompiledStream> {
        if self.assets.is_empty() {
            return Err(invalid(&self.root, "input contains no supported assets"));
        }
        // These types have no asset dependencies, so the SDK's stable sort
        // orders them by type name and then by case-insensitive instance name.
        self.assets
            .sort_by_cached_key(|asset| asset.name.to_ascii_lowercase());
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
        let assets = self
            .assets
            .iter()
            .map(|asset| {
                for reference in &asset.references {
                    if !self.ids.contains(reference) && !self.external_ids.contains(reference) {
                        return Err(invalid(
                            &self.root,
                            format!(
                                "unresolved reference {:08x}:{:08x} in {}",
                                reference.type_id, reference.instance_id, asset.name
                            ),
                        ));
                    }
                }
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
                Ok(AssetEntry {
                    id: asset.id,
                    type_hash: asset.type_hash,
                    instance_hash: asset.instance_hash,
                    chunk_sizes,
                    name: asset.name.as_bytes(),
                    source_file_name: asset.source.as_bytes(),
                    references: AssetReferences::from_ids(&asset.references),
                    tokenized: Some(0),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
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
        Ok(CompiledStream { manifest, files })
    }
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

fn hash_xml(node: Node<'_, '_>, processing_hash: u32, path: &Path) -> io::Result<u32> {
    // Match XmlNode.WriteTo on a standalone, empty element in the RA3 builder.
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"Windows-1252\"?><{}",
        node.tag_name().name()
    );
    for attr in node.attributes() {
        xml.push(' ');
        xml.push_str(attr.name());
        xml.push_str("=\"");
        for character in attr.value().chars() {
            match character {
                '&' => xml.push_str("&amp;"),
                '<' => xml.push_str("&lt;"),
                '>' => xml.push_str("&gt;"),
                '"' => xml.push_str("&quot;"),
                _ => xml.push(character),
            }
        }
        xml.push('"');
    }
    xml.push_str(" xmlns=\"uri:ea.com:eala:asset\" />");
    if xml.len() >= 512 {
        return Err(invalid(
            path,
            "asset declaration exceeds the supported hash length",
        ));
    }
    let seed = fast_hash_with_seed(processing_hash, b"11");
    Ok(fast_hash_with_seed(seed, xml.as_bytes()))
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
