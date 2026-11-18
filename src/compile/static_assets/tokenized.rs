use super::binary::BinaryData;
use super::{Marshaled, encode_words, fast_hash};
use crate::compile::invalid;
use std::io;
use std::path::Path;

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
    Null,
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
    pub(super) fn from_layout(layout: &[u8], data: &[u8], path: &Path) -> io::Result<Self> {
        fn read(bytes: &[u8], offset: usize, path: &Path) -> io::Result<u32> {
            bytes
                .get(offset..offset + 4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
                .ok_or_else(|| invalid(path, "truncated token layout"))
        }
        fn parse(
            layout: &[u8],
            data: &[u8],
            start: usize,
            chain: &mut Vec<usize>,
            path: &Path,
        ) -> io::Result<Table> {
            if chain.len() >= 128 || chain.contains(&start) || !start.is_multiple_of(12) {
                return Err(invalid(path, "cyclic or unaligned token layout"));
            }
            chain.push(start);
            let first = read(layout, start, path)?;
            let count = if first == COUNT {
                let pointer = read(layout, start + 8, path)? as usize;
                let offset = pointer
                    .checked_sub(layout.len())
                    .ok_or_else(|| invalid(path, "invalid token count pointer"))?;
                Some(read(data, offset, path)? as usize + 1)
            } else if first == 2 {
                Some(1)
            } else {
                None
            };
            let mut entries = Vec::new();
            let mut offset = start;
            loop {
                if count == Some(entries.len()) {
                    break;
                }
                let name = read(layout, offset, path)?;
                let kind = read(layout, offset + 4, path)?;
                let pointer = read(layout, offset + 8, path)? as usize;
                if count.is_none() && name == 0 && kind == 0 && pointer == 0 {
                    break;
                }
                let target = if pointer == 0 {
                    Target::Null
                } else if pointer >= layout.len() {
                    let target = pointer - layout.len();
                    if target >= data.len() {
                        return Err(invalid(path, "token data pointer is out of bounds"));
                    }
                    Target::Data(target)
                } else {
                    Target::Table(parse(layout, data, pointer, chain, path)?)
                };
                entries.push(Entry::new(name, kind, target));
                offset += 12;
            }
            chain.pop();
            Ok(Table {
                entries,
                terminated: count.is_none(),
            })
        }
        if layout.len() < 4
            || !(layout.len() - 4).is_multiple_of(12)
            || layout[layout.len() - 4..] != [0; 4]
        {
            return Err(invalid(path, "invalid token layout boundary"));
        }
        parse(layout, data, 0, &mut Vec::new(), path)
    }

    pub(super) fn sequence(entries: Vec<Entry>) -> Self {
        Self {
            entries,
            terminated: false,
        }
    }

    pub(super) fn data_offset(&self, name: &str) -> Option<usize> {
        let name = fast_hash(name.as_bytes());
        self.entries.iter().find_map(|entry| match entry.target {
            Target::Data(offset) if entry.name == name => Some(offset),
            _ => None,
        })
    }

    pub(super) fn replace_target(&mut self, name: &str, target: Target) -> bool {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.name == fast_hash(name.as_bytes()))
        {
            entry.target = target;
            true
        } else {
            false
        }
    }

    pub(super) fn derive_variant(&mut self, name: &str, kind: u32, bases: &[u32]) -> bool {
        let name = fast_hash(name.as_bytes());
        let Some(Target::Table(variants)) = self
            .entries
            .iter_mut()
            .find(|entry| entry.name == name)
            .map(|entry| &mut entry.target)
        else {
            return false;
        };
        let [variant] = variants.entries.as_mut_slice() else {
            return false;
        };
        variant.kind = kind;
        for &base in bases {
            let target = std::mem::replace(&mut variant.target, Target::Null);
            variant.target = Target::Table(Self::record(vec![Entry::new(1, base, target)]));
        }
        true
    }

    pub(super) fn shift_data_from(&mut self, threshold: usize, amount: usize) {
        for entry in &mut self.entries {
            match &mut entry.target {
                Target::Data(offset) if *offset >= threshold => *offset += amount,
                Target::Table(table) => table.shift_data_from(threshold, amount),
                _ => {}
            }
        }
    }
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
                Target::Null => 0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_polymorphic_tables_and_moves_their_data_targets() {
        let mut table = Table::record(vec![Entry::named("Body", 1, Target::Null)]);
        assert!(!table.replace_target("Missing", Target::Null));
        assert!(table.replace_target(
            "Body",
            Target::Table(Table::sequence(vec![Entry::new(
                2,
                3,
                Target::Table(Table::record(vec![Entry::new(4, 5, Target::Data(4)),]))
            ),]))
        ));
        table.shift_data_from(4, 8);
        let data = BinaryData {
            payload: vec![0; 16],
            relocations: Vec::new(),
            path: Path::new("insert"),
        };
        let result = finish(table, data, Marshaled::default()).unwrap();
        assert_eq!(
            &result.payload[..64],
            encode_words([
                fast_hash(b"Body"),
                1,
                24,
                0,
                0,
                0,
                2,
                3,
                36,
                4,
                5,
                76,
                0,
                0,
                0,
                0,
            ])
        );
    }

    #[test]
    fn rejects_cyclic_and_truncated_layouts() {
        let path = Path::new("invalid");
        assert!(Table::from_layout(&[0; 3], &[], path).is_err());
        let cycle = encode_words([1, 1, 12, 2, 1, 12, 0]);
        assert!(Table::from_layout(&cycle, &[], path).is_err());
    }

    #[test]
    fn rebuilds_game_object_token_layouts() {
        for (bytes, relocations, start) in [
            (
                include_bytes!("../../../tests/fixtures/game-object-bare.bin").as_slice(),
                include_bytes!("../../../tests/fixtures/game-object-bare.relo").as_slice(),
                0x7b4,
            ),
            (
                include_bytes!("../../../tests/fixtures/game-object-active-body.bin").as_slice(),
                include_bytes!("../../../tests/fixtures/game-object-active-body.relo").as_slice(),
                0x79c,
            ),
            (
                include_bytes!("../../../tests/fixtures/game-object-draw-minimal.bin").as_slice(),
                include_bytes!("../../../tests/fixtures/game-object-draw-minimal.relo").as_slice(),
                0xc1c,
            ),
        ] {
            let payload = &bytes[4..];
            let path = Path::new("oracle");
            let table = Table::from_layout(&payload[..start], &payload[start..], path).unwrap();
            let mut data = BinaryData {
                payload: payload[start..].to_vec(),
                relocations: Vec::new(),
                path,
            };
            for pointer in relocations[4..].as_chunks::<4>().0 {
                let pointer = u32::from_le_bytes(*pointer);
                if pointer == u32::MAX {
                    break;
                }
                let offset = pointer as usize - start;
                let target =
                    u32::from_le_bytes(data.payload[offset..offset + 4].try_into().unwrap());
                data.word(offset, target - start as u32);
                data.relocations.push(offset as u32);
            }
            let result = finish(table, data, Marshaled::default()).unwrap();
            assert_eq!(result.payload, payload);
            assert_eq!(encode_words(result.relocations), relocations[4..]);
        }
    }
}
