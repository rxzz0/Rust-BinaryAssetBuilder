//! Compiles the Red Alert 3 PC subset of AptUI XML backed by binary blobs.

use crate::hash::{fast_hash, fast_hash_with_seed};
use crate::linked_stream::LinkedFiles;
use crate::manifest::{AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest};
use roxmltree::{Document, Node};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SAGE_NS: &str = "uri:ea.com:eala:asset";
const FILE_HASH_BLOCK: usize = 0x100000;
const RA3_ALL_TYPES_HASH: u32 = 0x54ee_e764;
const RELOCATIONS: [u8; 8] = [4, 0, 0, 0, 255, 255, 255, 255];

pub struct CompiledStream {
    pub manifest: Vec<u8>,
    pub files: LinkedFiles,
}

/// Compiles a linked RA3 PC stream from AptUI declarations and relative includes.
pub fn compile_apt_ui(input: &Path, data_root: &Path) -> io::Result<CompiledStream> {
    let mut compiler = AptCompiler {
        root: fs::canonicalize(data_root)?,
        visited: HashSet::new(),
        active: HashSet::new(),
        ids: HashSet::new(),
        assets: Vec::new(),
        files: LinkedFiles {
            bin: vec![0; 4],
            relo: vec![0; 4],
            imp: vec![0; 4],
        },
    };
    compiler.read_document(&fs::canonicalize(input)?)?;
    compiler.finish()
}

struct AptCompiler {
    root: PathBuf,
    visited: HashSet<PathBuf>,
    active: HashSet<PathBuf>,
    ids: HashSet<AssetId>,
    assets: Vec<AptAsset>,
    files: LinkedFiles,
}

struct AptAsset {
    id: AssetId,
    type_hash: u32,
    instance_hash: u32,
    name: String,
    source: String,
    instance_size: u32,
}

impl AptCompiler {
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
                        || include.attribute("type") != Some("all")
                        || include.attributes().len() != 2
                        || has_content(include)
                    {
                        return Err(invalid(
                            path,
                            "only relative type=all includes are supported",
                        ));
                    }
                    let target = self.resolve(
                        path,
                        source.ok_or_else(|| invalid(path, "missing include source"))?,
                    )?;
                    self.read_document(&target)?;
                }
            } else {
                self.compile_asset(node, path, &source)?;
            }
        }
        self.active.remove(path);
        Ok(())
    }

    fn resolve(&self, document: &Path, value: &str) -> io::Result<PathBuf> {
        let value = value.trim().replace('\\', "/");
        let path = Path::new(&value);
        if value.is_empty() || value.contains(':') || path.has_root() {
            return Err(invalid(
                document,
                format!("expected relative path {value:?}"),
            ));
        }
        let path = document.with_file_name(path);
        let resolved = read_with_context(&path, fs::canonicalize)?;
        if !resolved.starts_with(&self.root) {
            return Err(invalid(&resolved, "path lies outside the data root"));
        }
        Ok(resolved)
    }

    fn compile_asset(&mut self, node: Node<'_, '_>, path: &Path, source: &str) -> io::Result<()> {
        let type_name = node.tag_name().name();
        let type_hash = match (node.tag_name().namespace(), type_name) {
            (Some(SAGE_NS), "AptAptData") => 0xe238_fac3,
            (Some(SAGE_NS), "AptConstData") => 0xe70a_e718,
            (Some(SAGE_NS), "AptDatData") => 0x0db6_1c30,
            (Some(SAGE_NS), "AptGeometryData") => 0x52dd_4bea,
            _ => return Err(invalid(path, format!("unsupported asset type {type_name}"))),
        };
        let geometry = type_name == "AptGeometryData";
        if node.attributes().len() != if geometry { 3 } else { 2 } || has_content(node) {
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
        if !self.ids.insert(asset_id) {
            return Err(invalid(path, format!("duplicate asset ID {name}")));
        }

        let bytes = read_with_context(&self.resolve(path, file)?, fs::read)?;
        let length = u32::try_from(bytes.len()).map_err(|_| invalid(path, "blob is too large"))?;
        let header_size = if geometry { 16u32 } else { 12u32 };
        let instance_size = length
            .checked_next_multiple_of(4)
            .and_then(|size| size.checked_add(header_size))
            .filter(|&size| size <= i32::MAX as u32)
            .ok_or_else(|| invalid(path, "blob is too large"))?;
        let instance_hash = hash_asset(node, type_hash, &bytes, length, path)?;
        self.files.bin.extend(0u32.to_le_bytes());
        self.files.bin.extend(header_size.to_le_bytes());
        self.files.bin.extend(length.to_le_bytes());
        if let Some(id) = geometry_id {
            self.files.bin.extend(id.to_le_bytes());
        }
        self.files.bin.extend(bytes);
        self.files.bin.resize(
            self.files.bin.len() + (length.next_multiple_of(4) - length) as usize,
            0,
        );
        self.files.relo.extend(RELOCATIONS);
        self.assets.push(AptAsset {
            id: asset_id,
            type_hash,
            instance_hash,
            name,
            source: source.to_owned(),
            instance_size,
        });
        Ok(())
    }

    fn finish(mut self) -> io::Result<CompiledStream> {
        if self.assets.is_empty() {
            return Err(invalid(
                &self.root,
                "input contains no supported AptUI assets",
            ));
        }
        let mut checksums = Vec::new();
        let mut max_instance = 0;
        let assets = self
            .assets
            .iter()
            .map(|asset| {
                for word in [
                    asset.id.type_id,
                    asset.type_hash,
                    asset.id.instance_id,
                    asset.instance_hash,
                    0,
                ] {
                    checksums.extend(word.to_le_bytes());
                }
                max_instance = max_instance.max(asset.instance_size);
                AssetEntry {
                    id: asset.id,
                    type_hash: asset.type_hash,
                    instance_hash: asset.instance_hash,
                    chunk_sizes: ChunkSizes {
                        instance: asset.instance_size,
                        relocations: 8,
                        imports: 0,
                    },
                    name: asset.name.as_bytes(),
                    source_file_name: asset.source.as_bytes(),
                    references: AssetReferences::from_ids(&[]),
                    tokenized: Some(0),
                }
            })
            .collect();
        let stream_checksum = memory_stream_hash(checksums);
        for output in [
            &mut self.files.bin,
            &mut self.files.relo,
            &mut self.files.imp,
        ] {
            output[..4].copy_from_slice(&stream_checksum.to_le_bytes());
        }
        let manifest = Manifest {
            version: 6,
            byte_order: ByteOrder::Little,
            is_linked: true,
            stream_checksum,
            all_types_hash: RA3_ALL_TYPES_HASH,
            total_instance_data_size: u32::try_from(self.files.bin.len() - 4)
                .map_err(|_| invalid(&self.root, "stream is too large"))?,
            max_chunk_sizes: ChunkSizes {
                instance: max_instance,
                relocations: 8,
                imports: 0,
            },
            assets,
            referenced_manifests: Vec::new(),
        }
        .encode()
        .map_err(io::Error::other)?;
        Ok(CompiledStream {
            manifest,
            files: self.files,
        })
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
    type_hash: u32,
    bytes: &[u8],
    length: u32,
    path: &Path,
) -> io::Result<u32> {
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
            "AptUI declaration exceeds the supported hash length",
        ));
    }
    let seed = fast_hash_with_seed(type_hash ^ 4, b"11");
    let xml_hash = fast_hash_with_seed(seed, xml.as_bytes());
    let file_hash = bytes
        .chunks(FILE_HASH_BLOCK)
        .fold(length, fast_hash_with_seed);
    Ok(xml_hash ^ memory_stream_hash(file_hash.to_le_bytes().to_vec()))
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
