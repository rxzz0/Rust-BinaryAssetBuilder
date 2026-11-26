use super::{escape_xml, invalid, windows_1252};
use crate::hash::{asset_string_hash, fast_hash};
use std::collections::{BTreeMap, btree_map::Entry};
use std::fmt::Write;
use std::io;
use std::path::Path;

#[derive(Default)]
pub(super) struct StringHashes {
    bins: [BTreeMap<u32, String>; 4],
}

impl StringHashes {
    pub(super) fn asset(&mut self, kind: &str, name: &str, path: &Path) -> io::Result<()> {
        self.insert(
            0,
            fast_hash(name.to_ascii_lowercase().as_bytes()),
            name,
            path,
        )?;
        self.insert(1, fast_hash(kind.as_bytes()), kind, path)
    }

    pub(super) fn value(&mut self, kind: &str, value: &str, path: &Path) -> io::Result<()> {
        let slot = if kind == "Poid" { 2 } else { 3 };
        let key = value.to_lowercase();
        let bytes = windows_1252(&key)
            .ok_or_else(|| invalid(path, format!("invalid Windows-1252 {kind} value {value:?}")))?;
        self.insert(slot, asset_string_hash(&bytes), value, path)
    }

    fn insert(&mut self, slot: usize, hash: u32, text: &str, path: &Path) -> io::Result<()> {
        match self.bins[slot].entry(hash) {
            Entry::Vacant(entry) => {
                entry.insert(text.to_owned());
            }
            Entry::Occupied(entry) if entry.get().to_lowercase() != text.to_lowercase() => {
                return Err(invalid(
                    path,
                    format!(
                        "hash collision for {:?} and {text:?} (0x{hash:08x})",
                        entry.get()
                    ),
                ));
            }
            Entry::Occupied(_) => {}
        }
        Ok(())
    }

    pub(super) fn xml(&self) -> String {
        let mut xml = String::from(
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<AssetDeclaration xmlns=\"uri:ea.com:eala:asset\">",
        );
        let version = fast_hash(b"StringHashTable") ^ 2;
        for (name, bin) in ["INSTANCEID", "TYPEID", "POID", "STRINGHASH"]
            .into_iter()
            .zip(&self.bins)
        {
            write!(xml, "\r\n  <StringHashTable Version=\"{version}\" id=\"StringHashBin_{name}\" StringHashBin=\"{name}\"").unwrap();
            if bin.is_empty() {
                xml.push_str(" />");
                continue;
            }
            xml.push('>');
            for (hash, value) in bin {
                write!(xml, "\r\n    <StringAndHash Hash=\"{hash}\" Text=\"").unwrap();
                escape_xml(value, true, &mut xml);
                xml.push_str("\" />");
            }
            xml.push_str("\r\n  </StringHashTable>");
        }
        xml.push_str("\r\n</AssetDeclaration>");
        xml
    }
}
