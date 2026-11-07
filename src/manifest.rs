//! Reader for the version 5 SAGE stream manifest used by the upstream build.

use std::fmt;

const VERSION: u16 = 5;
const HEADER_SIZE: usize = 48;
const ENTRY_SIZE: usize = 44;
const ASSET_ID_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn u16(self, bytes: [u8; 2]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        }
    }

    pub(crate) fn u32(self, bytes: [u8; 4]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes(bytes),
            Self::Big => u32::from_be_bytes(bytes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AssetId {
    pub type_id: u32,
    pub instance_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChunkSizes {
    pub instance: u32,
    pub relocations: u32,
    pub imports: u32,
}

#[derive(Debug)]
pub struct Manifest<'a> {
    pub byte_order: ByteOrder,
    pub is_linked: bool,
    pub stream_checksum: u32,
    pub all_types_hash: u32,
    pub total_instance_data_size: u32,
    pub max_chunk_sizes: ChunkSizes,
    pub assets: Vec<AssetEntry<'a>>,
    pub referenced_manifests: Vec<ReferencedManifest<'a>>,
}

#[derive(Debug)]
pub struct AssetEntry<'a> {
    pub id: AssetId,
    pub type_hash: u32,
    pub instance_hash: u32,
    pub chunk_sizes: ChunkSizes,
    /// Null-terminated ANSI strings are exposed as bytes, without the terminator.
    pub name: &'a [u8],
    pub source_file_name: &'a [u8],
    pub references: AssetReferences<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct AssetReferences<'a> {
    bytes: &'a [u8],
    byte_order: ByteOrder,
}

impl AssetReferences<'_> {
    pub fn iter(&self) -> impl ExactSizeIterator<Item = AssetId> + '_ {
        let (ids, _) = self.bytes.as_chunks::<ASSET_ID_SIZE>();
        ids.iter().map(|id| AssetId {
            type_id: self.byte_order.u32([id[0], id[1], id[2], id[3]]),
            instance_id: self.byte_order.u32([id[4], id[5], id[6], id[7]]),
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct ReferencedManifest<'a> {
    pub path: &'a [u8],
    pub is_patch: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ManifestError {
    UnsupportedVersion(u16),
    Truncated(&'static str),
    Invalid(&'static str),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported manifest version {version}")
            }
            Self::Truncated(section) => write!(f, "truncated manifest {section}"),
            Self::Invalid(field) => write!(f, "invalid manifest {field}"),
        }
    }
}

impl std::error::Error for ManifestError {}

impl<'a> Manifest<'a> {
    pub fn parse(mut bytes: &'a [u8]) -> Result<Self, ManifestError> {
        let header = take(&mut bytes, HEADER_SIZE, "header")?;
        let byte_order = match header[0] {
            0 => ByteOrder::Little,
            _ => ByteOrder::Big,
        };
        let version = byte_order.u16([header[2], header[3]]);
        if version != VERSION {
            return Err(ManifestError::UnsupportedVersion(version));
        }
        let (words, _) = header[4..].as_chunks::<4>();
        let word = |index: usize| byte_order.u32(words[index]);
        let entries_size = (word(2) as usize)
            .checked_mul(ENTRY_SIZE)
            .ok_or(ManifestError::Invalid("asset count"))?;
        let entries = take(&mut bytes, entries_size, "asset entries")?;
        let references = take(&mut bytes, word(7) as usize, "asset references")?;
        if !references.len().is_multiple_of(ASSET_ID_SIZE) {
            return Err(ManifestError::Invalid("asset reference buffer size"));
        }
        let manifest_names = take(&mut bytes, word(8) as usize, "referenced manifest names")?;
        let asset_names = take(&mut bytes, word(9) as usize, "asset names")?;
        let source_names = take(&mut bytes, word(10) as usize, "source file names")?;
        if !bytes.is_empty() {
            return Err(ManifestError::Invalid("trailing bytes"));
        }

        let (entries, _) = entries.as_chunks::<ENTRY_SIZE>();
        let assets = entries
            .iter()
            .map(|entry| parse_entry(entry, byte_order, references, asset_names, source_names))
            .collect::<Result<_, _>>()?;

        Ok(Self {
            byte_order,
            is_linked: header[1] != 0,
            stream_checksum: word(0),
            all_types_hash: word(1),
            total_instance_data_size: word(3),
            max_chunk_sizes: ChunkSizes {
                instance: word(4),
                relocations: word(5),
                imports: word(6),
            },
            assets,
            referenced_manifests: parse_manifest_names(manifest_names)?,
        })
    }
}

fn parse_entry<'a>(
    entry: &[u8; ENTRY_SIZE],
    byte_order: ByteOrder,
    references: &'a [u8],
    asset_names: &'a [u8],
    source_names: &'a [u8],
) -> Result<AssetEntry<'a>, ManifestError> {
    let (words, _) = entry.as_chunks::<4>();
    let word = |index: usize| byte_order.u32(words[index]);
    let field =
        |index, name| u32::try_from(word(index) as i32).map_err(|_| ManifestError::Invalid(name));
    let offset = field(4, "asset reference offset")? as usize;
    let count = field(5, "asset reference count")? as usize;
    let reference_size = count
        .checked_mul(ASSET_ID_SIZE)
        .ok_or(ManifestError::Invalid("asset reference count"))?;
    let references = references
        .get(offset..)
        .filter(|_| offset.is_multiple_of(ASSET_ID_SIZE))
        .and_then(|bytes| bytes.get(..reference_size))
        .ok_or(ManifestError::Invalid("asset reference range"))?;

    Ok(AssetEntry {
        id: AssetId {
            type_id: word(0),
            instance_id: word(1),
        },
        type_hash: word(2),
        instance_hash: word(3),
        chunk_sizes: ChunkSizes {
            instance: field(8, "instance data size")?,
            relocations: field(9, "relocation data size")?,
            imports: field(10, "import data size")?,
        },
        name: name_at(asset_names, field(6, "asset name offset")?, "asset name")?,
        source_file_name: name_at(
            source_names,
            field(7, "source file name offset")?,
            "source file name",
        )?,
        references: AssetReferences {
            bytes: references,
            byte_order,
        },
    })
}

fn parse_manifest_names(mut bytes: &[u8]) -> Result<Vec<ReferencedManifest<'_>>, ManifestError> {
    let mut manifests = Vec::new();
    while let Some((&kind, tail)) = bytes.split_first() {
        let is_patch = match kind {
            1 => false,
            2 => true,
            _ => return Err(ManifestError::Invalid("reference kind")),
        };
        let (path, rest) =
            nul_string(tail).ok_or(ManifestError::Invalid("referenced manifest path"))?;
        manifests.push(ReferencedManifest { path, is_patch });
        bytes = rest;
    }
    Ok(manifests)
}

fn name_at<'a>(
    bytes: &'a [u8],
    offset: u32,
    field: &'static str,
) -> Result<&'a [u8], ManifestError> {
    bytes
        .get(offset as usize..)
        .and_then(nul_string)
        .map(|(name, _)| name)
        .ok_or(ManifestError::Invalid(field))
}

fn nul_string(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let end = bytes.iter().position(|&byte| byte == 0)?;
    Some((&bytes[..end], &bytes[end + 1..]))
}

fn take<'a>(
    bytes: &mut &'a [u8],
    size: usize,
    section: &'static str,
) -> Result<&'a [u8], ManifestError> {
    let (section_bytes, rest) = bytes
        .split_at_checked(size)
        .ok_or(ManifestError::Truncated(section))?;
    *bytes = rest;
    Ok(section_bytes)
}
