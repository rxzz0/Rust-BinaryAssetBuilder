use super::binary::BinaryData;
use super::{Marshaled, encode_words, fast_hash};
use crate::compile::invalid;
use std::io;

const COUNT: u32 = 0x6c82_7073;
const U32_TYPE: u32 = 0x5d44_8fd8;
const ITEM: u32 = 0xb151_a0eb;
pub(super) const FLOAT_TYPE: u32 = 0x6458_64f3;

pub(super) struct Table {
    entries: Vec<Entry>,
    terminated: bool,
}

pub(super) struct Entry {
    name: u32,
    kind: u32,
    target: Target,
}

pub(super) enum Target {
    Data(usize),
    Table(Table),
}

impl Entry {
    pub(super) fn named(name: &str, kind: u32, target: Target) -> Self {
        Self::new(fast_hash(name.as_bytes()), kind, target)
    }

    pub(super) fn new(name: u32, kind: u32, target: Target) -> Self {
        Self { name, kind, target }
    }
}

impl Table {
    pub(super) fn record(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            terminated: true,
        }
    }

    pub(super) fn array(count: usize, items: impl IntoIterator<Item = (u32, Target)>) -> Self {
        let mut entries = vec![Entry::new(COUNT, U32_TYPE, Target::Data(count))];
        entries.extend(
            items
                .into_iter()
                .map(|(kind, target)| Entry::new(ITEM, kind, target)),
        );
        Self {
            entries,
            terminated: false,
        }
    }

    fn write(self, words: &mut Vec<u32>, data_offsets: &mut Vec<usize>) -> usize {
        let start = words.len();
        words.resize(
            start + 3 * (self.entries.len() + usize::from(self.terminated)),
            0,
        );
        for (index, entry) in self.entries.into_iter().enumerate() {
            let offset = start + 3 * index;
            words[offset] = entry.name;
            words[offset + 1] = entry.kind;
            words[offset + 2] = match entry.target {
                Target::Data(target) => {
                    data_offsets.push(offset + 2);
                    target as u32
                }
                Target::Table(table) => table.write(words, data_offsets) as u32,
            };
        }
        4 * start
    }
}

pub(super) fn finish(
    table: Table,
    mut data: BinaryData<'_>,
    mut asset: Marshaled,
) -> io::Result<Marshaled> {
    let mut words = Vec::new();
    let mut data_offsets = Vec::new();
    table.write(&mut words, &mut data_offsets);
    words.push(0);
    let data_start = u32::try_from(4 * words.len())
        .ok()
        .filter(|start| *start as usize + data.payload.len() <= i32::MAX as usize)
        .ok_or_else(|| invalid(data.path, "tokenized asset is too large"))?;
    for offset in data_offsets {
        words[offset] += data_start;
    }
    for pointer in &mut data.relocations {
        let offset = *pointer as usize;
        let target = u32::from_le_bytes(data.payload[offset..offset + 4].try_into().unwrap());
        data.payload[offset..offset + 4].copy_from_slice(&(target + data_start).to_le_bytes());
        *pointer += data_start;
    }
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    let mut payload = encode_words(words);
    payload.extend_from_slice(&data.payload);
    asset.payload = payload;
    asset.relocations = data.relocations;
    asset.tokenized = true;
    Ok(asset)
}
