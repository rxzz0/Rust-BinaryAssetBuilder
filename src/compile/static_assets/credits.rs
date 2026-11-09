use super::{Marshaled, color, required_element, significant_children, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid, is_xml_whitespace, required, windows_1252};
use roxmltree::Node;
use std::io;
use std::path::Path;

const STYLE_SIZE: usize = 24;
const ITEM_SIZE: usize = 12;
const STRING_SIZE: usize = 8;
const STYLES: [&str; 4] = ["TitleFont", "MinorTitleFont", "NormalFont", "ColumnFont"];

pub(super) fn credits(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node).peekable();
    let scroll = required_element(&mut children, "Scroll", path)?;
    validate_attributes(scroll, &["Rate", "RateEveryFrame", "ScrollDown"], path)?;
    if has_content(scroll) {
        return Err(invalid(path, "unexpected content in Scroll"));
    }
    let mut binary = CreditsData {
        payload: vec![0; 32],
        relocations: Vec::new(),
        path,
    };
    binary.word(4, unsigned(scroll, "Rate", path)?);
    binary.word(8, unsigned(scroll, "RateEveryFrame", path)?);
    binary.word(12, boolean(required(scroll, "ScrollDown", path)?, path)?);

    let mut styles = Vec::new();
    while let Some(style) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Style"))) {
        styles.push(style);
    }
    let style_start = binary.list(16, styles.len(), STYLE_SIZE)?;
    for (index, style) in styles.into_iter().enumerate() {
        let offset = style_start + index * STYLE_SIZE;
        validate_attributes(style, &["Name"], path)?;
        binary.word(offset, style_index(required(style, "Name", path)?, path)?);
        let mut fields = significant_children(style);
        let rgb = required_element(&mut fields, "Color", path)?;
        binary.word(offset + 4, color(rgb, &["B", "G", "R"], 255, path)?);
        let font = required_element(&mut fields, "Font", path)?;
        validate_attributes(font, &["Name", "Size", "Bold"], path)?;
        if has_content(font) || fields.next().is_some() {
            return Err(invalid(path, "unexpected content in credit Style"));
        }
        let name = font
            .attribute("Name")
            .ok_or_else(|| invalid(path, "missing Font Name"))?;
        binary.string(offset + 8, name)?;
        binary.word(offset + 16, unsigned(font, "Size", path)?);
        binary.word(
            offset + 20,
            boolean(font.attribute("Bold").unwrap_or("false"), path)?,
        );
    }

    let items: Vec<_> = children.collect();
    let item_start = binary.list(24, items.len(), ITEM_SIZE)?;
    for (index, item) in items.into_iter().enumerate() {
        if !item.has_tag_name((SAGE_NS, "CreditItem")) {
            return Err(invalid(path, "expected a CreditItem"));
        }
        validate_attributes(item, &["Style"], path)?;
        let offset = item_start + index * ITEM_SIZE;
        binary.word(offset, style_index(required(item, "Style", path)?, path)?);
        let lines: Vec<_> = significant_children(item).collect();
        let line_start = binary.list(offset + 4, lines.len(), STRING_SIZE)?;
        for (index, line) in lines.into_iter().enumerate() {
            if !line.has_tag_name((SAGE_NS, "Text"))
                || line.children().any(|child| child.is_element())
            {
                return Err(invalid(path, "expected a credit Text element"));
            }
            validate_attributes(line, &[], path)?;
            let text: String = line
                .children()
                .filter(Node::is_text)
                .filter_map(|child| child.text())
                .collect();
            let text = if is_xml_whitespace(&text) { "" } else { &text };
            binary.string(line_start + index * STRING_SIZE, text)?;
        }
    }
    binary.relocations.sort_unstable();
    if !binary.relocations.is_empty() {
        binary.relocations.push(u32::MAX);
    }
    Ok(Marshaled {
        payload: binary.payload,
        relocations: binary.relocations,
        ..Marshaled::default()
    })
}

struct CreditsData<'a> {
    payload: Vec<u8>,
    relocations: Vec<u32>,
    path: &'a Path,
}

impl CreditsData<'_> {
    fn word(&mut self, offset: usize, word: u32) {
        self.payload[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }

    fn list(&mut self, offset: usize, count: usize, item_size: usize) -> io::Result<usize> {
        if count == 0 {
            return Ok(0);
        }
        let bytes = count
            .checked_mul(item_size)
            .ok_or_else(|| invalid(self.path, "Credits list is too large"))?;
        let start = self.allocate(offset + 4, bytes)?;
        self.word(offset, count as u32);
        Ok(start)
    }

    fn string(&mut self, offset: usize, text: &str) -> io::Result<()> {
        let text = windows_1252(text).ok_or_else(|| {
            invalid(
                self.path,
                "Credits text contains characters outside Windows-1252",
            )
        })?;
        let size = text
            .len()
            .checked_add(1)
            .and_then(|size| size.checked_next_multiple_of(4))
            .ok_or_else(|| invalid(self.path, "Credits string is too large"))?;
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
            .ok_or_else(|| invalid(self.path, "Credits payload is too large"))?;
        self.payload.resize(end, 0);
        self.word(pointer, start as u32);
        self.relocations.push(pointer as u32);
        Ok(start)
    }
}

fn unsigned(node: Node<'_, '_>, field: &str, path: &Path) -> io::Result<u32> {
    required(node, field, path)?
        .trim()
        .parse()
        .map_err(|_| invalid(path, format!("invalid Credits {field}")))
}

fn boolean(value: &str, path: &Path) -> io::Result<u32> {
    match value.trim() {
        "false" | "0" => Ok(0),
        "true" | "1" => Ok(1),
        _ => Err(invalid(path, "invalid Credits boolean")),
    }
}

fn style_index(name: &str, path: &Path) -> io::Result<u32> {
    STYLES
        .iter()
        .position(|&style| style == name)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("unknown Credits style {name:?}")))
}
