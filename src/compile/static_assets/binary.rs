use crate::compile::{invalid, windows_1252};
use std::io;
use std::path::Path;

pub(super) struct BinaryData<'a> {
    pub(super) payload: Vec<u8>,
    pub(super) relocations: Vec<u32>,
    pub(super) path: &'a Path,
}

impl BinaryData<'_> {
    pub(super) fn word(&mut self, offset: usize, word: u32) {
        self.payload[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    pub(super) fn list(
        &mut self,
        offset: usize,
        count: usize,
        item_size: usize,
    ) -> io::Result<usize> {
        if count == 0 {
            return Ok(0);
        }
        let bytes = count
            .checked_mul(item_size)
            .ok_or_else(|| invalid(self.path, "Asset list is too large"))?;
        let start = self.allocate(offset + 4, bytes)?;
        self.word(offset, count as u32);
        Ok(start)
    }

    pub(super) fn string(&mut self, offset: usize, text: &str) -> io::Result<()> {
        let text = windows_1252(text).ok_or_else(|| {
            invalid(
                self.path,
                "Asset text contains characters outside Windows-1252",
            )
        })?;
        let size = text
            .len()
            .checked_add(1)
            .and_then(|size| size.checked_next_multiple_of(4))
            .ok_or_else(|| invalid(self.path, "Asset string is too large"))?;
        let start = self.allocate(offset + 4, size)?;
        self.word(offset, text.len() as u32);
        self.payload[start..start + text.len()].copy_from_slice(&text);
        Ok(())
    }

    fn allocate(&mut self, pointer: usize, bytes: usize) -> io::Result<usize> {
        let start = self.payload.len();
        let end = start
            .checked_add(bytes)
            .filter(|&end| end <= i32::MAX as usize)
            .ok_or_else(|| invalid(self.path, "Asset payload is too large"))?;
        self.payload.resize(end, 0);
        self.word(pointer, start as u32);
        self.relocations.push(pointer as u32);
        Ok(start)
    }
}
