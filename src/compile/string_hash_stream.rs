use super::{
    CompiledStream, RA3_ALL_TYPES_HASH, hash_xml, invalid, memory_stream_hash, windows_1252,
};
use crate::hash::fast_hash;
use crate::linked_stream::LinkedFiles;
use crate::manifest::{AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;

const TYPE_ID: u32 = 0x11e5_cf64;
const TYPE_HASH: u32 = 0xebfe_c3eb;
const TABLES: [(&str, u32); 4] = [
    ("INSTANCEID", 1),
    ("POID", 2),
    ("STRINGHASH", 3),
    ("TYPEID", 0),
];

pub(super) fn encode(xml: &str, source: &str, path: &Path) -> io::Result<CompiledStream> {
    let document = Document::parse(xml).map_err(|error| invalid(path, error))?;
    let declaration = document.root_element();
    let mut files = LinkedFiles {
        bin: vec![0; 4],
        relo: vec![0; 4],
        imp: vec![0; 4],
    };
    let mut max_chunk_sizes = ChunkSizes {
        instance: 0,
        relocations: 0,
        imports: 0,
    };
    let mut checksums = Vec::with_capacity(TABLES.len() * 20);
    let mut entries = Vec::with_capacity(TABLES.len());
    let names = TABLES.map(|(kind, _)| format!("StringHashTable:StringHashBin_{kind}"));

    for ((kind, bin_id), name) in TABLES.into_iter().zip(&names) {
        let (type_name, instance_name) = name.split_once(':').expect("qualified table name");
        let node = declaration
            .children()
            .find(|child| {
                child.has_tag_name(type_name)
                    && child.attribute("id") == Some(instance_name)
                    && child.attribute("StringHashBin") == Some(kind)
            })
            .ok_or_else(|| invalid(path, format!("missing {name}")))?;
        let instance_id = fast_hash(instance_name.to_ascii_lowercase().as_bytes());
        let instance_hash = hash_xml(node, TYPE_HASH ^ 4, path)?;
        let (payload, relocations) = encode_table(node, bin_id, path)?;
        let chunk_sizes = ChunkSizes {
            instance: stream_size(payload.len(), path)?,
            relocations: stream_size(relocations.len(), path)?,
            imports: 0,
        };
        files.bin.extend_from_slice(&payload);
        files.relo.extend_from_slice(&relocations);
        max_chunk_sizes.instance = max_chunk_sizes.instance.max(chunk_sizes.instance);
        max_chunk_sizes.relocations = max_chunk_sizes.relocations.max(chunk_sizes.relocations);
        push_words(
            &mut checksums,
            [TYPE_ID, TYPE_HASH, instance_id, instance_hash, 0],
        );
        entries.push(AssetEntry {
            id: AssetId {
                type_id: TYPE_ID,
                instance_id,
            },
            type_hash: TYPE_HASH,
            instance_hash,
            chunk_sizes,
            name: name.as_bytes(),
            source_file_name: source.as_bytes(),
            references: AssetReferences::from_ids(&[]),
            tokenized: Some(0),
        });
    }

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
        total_instance_data_size: stream_size(files.bin.len() - 4, path)?,
        max_chunk_sizes,
        assets: entries,
        referenced_manifests: Vec::new(),
    }
    .encode()
    .map_err(io::Error::other)?;
    Ok(CompiledStream {
        manifest,
        files,
        custom_data: Vec::new(),
    })
}

fn encode_table(node: Node<'_, '_>, bin_id: u32, path: &Path) -> io::Result<(Vec<u8>, Vec<u8>)> {
    const HEADER_SIZE: usize = 16;
    const RECORD_SIZE: usize = 12;
    let records = node.children().filter(Node::is_element);
    let count = records.clone().count();
    let mut payload = Vec::new();
    let mut relocations = Vec::new();
    push_words(
        &mut payload,
        [
            0,
            bin_id,
            stream_size(count, path)?,
            if count == 0 { 0 } else { HEADER_SIZE as u32 },
        ],
    );
    if count == 0 {
        return Ok((payload, relocations));
    }

    payload.resize(HEADER_SIZE + RECORD_SIZE * count, 0);
    push_words(&mut relocations, [12]);
    for (index, record) in records.enumerate() {
        let hash = record
            .attribute("Hash")
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| invalid(path, "StringAndHash requires a numeric Hash"))?;
        let text = record
            .attribute("Text")
            .and_then(windows_1252)
            .ok_or_else(|| invalid(path, "StringAndHash requires Windows-1252 Text"))?;
        let offset = HEADER_SIZE + index * RECORD_SIZE;
        let words = [
            hash,
            stream_size(text.len(), path)?,
            stream_size(payload.len(), path)?,
        ];
        for (field, value) in payload[offset..offset + RECORD_SIZE]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(words)
        {
            field.copy_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&text);
        payload.push(0);
        payload.resize(payload.len().next_multiple_of(4), 0);
        push_words(&mut relocations, [stream_size(offset + 8, path)?]);
    }
    push_words(&mut relocations, [u32::MAX]);
    Ok((payload, relocations))
}

fn stream_size(size: usize, path: &Path) -> io::Result<u32> {
    u32::try_from(size).map_err(|_| invalid(path, "StringHashTable stream is too large"))
}

fn push_words<const N: usize>(bytes: &mut Vec<u8>, words: [u32; N]) {
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
}
