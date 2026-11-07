use super::{ByteOrder, HEADER_SIZE, Manifest, ManifestError, VERSION};
use std::collections::HashMap;

impl Manifest<'_> {
    /// Encodes a version 5 manifest, preserving asset and referenced stream order.
    pub fn encode(&self) -> Result<Vec<u8>, ManifestError> {
        let mut output = vec![0; HEADER_SIZE];
        let mut references = Vec::new();
        let mut asset_names = NameBuffer::default();
        let mut source_names = NameBuffer::default();

        for asset in &self.assets {
            let reference_offset = entry_field(references.len(), "asset reference offset")?;
            let ids = asset.references.iter();
            let reference_count = entry_field(ids.len(), "asset reference count")?;
            for id in ids {
                for value in [id.type_id, id.instance_id] {
                    references.extend_from_slice(&self.byte_order.u32_bytes(value));
                }
            }

            let name_offset = asset_names.add(asset.name, "asset name")?;
            let source_offset = source_names.add(asset.source_file_name, "source file name")?;
            for value in [
                asset.id.type_id,
                asset.id.instance_id,
                asset.type_hash,
                asset.instance_hash,
                reference_offset,
                reference_count,
                name_offset,
                source_offset,
                entry_field(asset.chunk_sizes.instance as usize, "instance data size")?,
                entry_field(
                    asset.chunk_sizes.relocations as usize,
                    "relocation data size",
                )?,
                entry_field(asset.chunk_sizes.imports as usize, "import data size")?,
            ] {
                output.extend_from_slice(&self.byte_order.u32_bytes(value));
            }
        }

        let mut manifest_names = Vec::new();
        for reference in &self.referenced_manifests {
            manifest_names.push(if reference.is_patch { 2 } else { 1 });
            push_name(
                &mut manifest_names,
                reference.path,
                "referenced manifest path",
            )?;
        }

        output[0] = u8::from(self.byte_order == ByteOrder::Big);
        output[1] = u8::from(self.is_linked);
        output[2..4].copy_from_slice(&match self.byte_order {
            ByteOrder::Little => VERSION.to_le_bytes(),
            ByteOrder::Big => VERSION.to_be_bytes(),
        });
        let header_words = [
            self.stream_checksum,
            self.all_types_hash,
            header_field(self.assets.len(), "asset count")?,
            self.total_instance_data_size,
            self.max_chunk_sizes.instance,
            self.max_chunk_sizes.relocations,
            self.max_chunk_sizes.imports,
            header_field(references.len(), "asset reference buffer size")?,
            header_field(manifest_names.len(), "referenced manifest names size")?,
            header_field(asset_names.bytes.len(), "asset names size")?,
            header_field(source_names.bytes.len(), "source file names size")?,
        ];
        let (words, _) = output[4..HEADER_SIZE].as_chunks_mut::<4>();
        for (word, value) in words.iter_mut().zip(header_words) {
            *word = self.byte_order.u32_bytes(value);
        }

        output.extend(references);
        output.extend(manifest_names);
        output.extend(asset_names.bytes);
        output.extend(source_names.bytes);
        Ok(output)
    }
}

impl ByteOrder {
    pub(crate) fn u32_bytes(self, value: u32) -> [u8; 4] {
        match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        }
    }
}

#[derive(Default)]
struct NameBuffer<'a> {
    bytes: Vec<u8>,
    offsets: HashMap<&'a [u8], u32>,
}

impl<'a> NameBuffer<'a> {
    fn add(&mut self, name: &'a [u8], field: &'static str) -> Result<u32, ManifestError> {
        if let Some(&offset) = self.offsets.get(name) {
            return Ok(offset);
        }
        let offset = entry_field(self.bytes.len(), field)?;
        push_name(&mut self.bytes, name, field)?;
        self.offsets.insert(name, offset);
        Ok(offset)
    }
}

fn push_name(output: &mut Vec<u8>, name: &[u8], field: &'static str) -> Result<(), ManifestError> {
    if name.contains(&0) {
        return Err(ManifestError::Invalid(field));
    }
    output.extend_from_slice(name);
    output.push(0);
    Ok(())
}

fn entry_field(value: usize, field: &'static str) -> Result<u32, ManifestError> {
    i32::try_from(value)
        .map(|value| value as u32)
        .map_err(|_| ManifestError::Invalid(field))
}

fn header_field(value: usize, field: &'static str) -> Result<u32, ManifestError> {
    u32::try_from(value).map_err(|_| ManifestError::Invalid(field))
}
