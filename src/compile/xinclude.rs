use super::{Compiler, XML_NS, escape_xml, invalid, opening_tag_end, read_with_context, required};
use roxmltree::Document;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const XI_NS: &str = "http://www.w3.org/2001/XInclude";

impl Compiler<'_> {
    pub(super) fn read_static_xml(&mut self, path: &Path) -> io::Result<Rc<str>> {
        let xml = read_with_context(path, fs::read_to_string)?.into();
        self.expand_xml_includes(path, xml, &mut vec![path.to_path_buf()])
    }

    fn expand_xml_includes(
        &mut self,
        path: &Path,
        xml: Rc<str>,
        chain: &mut Vec<PathBuf>,
    ) -> io::Result<Rc<str>> {
        if !xml.contains(XI_NS) {
            return Ok(xml);
        }
        let document = Document::parse(&xml).map_err(|error| invalid(path, error))?;
        let mut includes = document
            .descendants()
            .filter(|node| node.tag_name().namespace() == Some(XI_NS))
            .peekable();
        if includes.peek().is_none() {
            return Ok(xml);
        }
        let mut expanded = String::with_capacity(xml.len());
        let mut copied = 0;
        for include in includes {
            if !include.has_tag_name((XI_NS, "include"))
                || include.attributes().any(|attr| {
                    attr.namespace().is_some() || !matches!(attr.name(), "href" | "parse")
                })
                || !matches!(include.attribute("parse"), None | Some("xml"))
                || include.children().any(|child| {
                    child.is_element()
                        || child.is_text()
                            && child.text().is_some_and(|text| !text.trim().is_empty())
                })
            {
                return Err(invalid(path, "expected xi:include of a whole XML fragment"));
            }
            let href = required(include, "href", path)?;
            if href.contains('#') {
                return Err(invalid(path, "XInclude fragment selectors are unsupported"));
            }
            let target = self.resolve(path, href)?;
            let fragment = self.xml_fragment(path, &target, href, chain)?;
            let range = include.range();
            expanded.push_str(&xml[copied..range.start]);
            expanded.push_str(&fragment);
            copied = range.end;
        }
        expanded.push_str(&xml[copied..]);
        Ok(expanded.into())
    }

    fn xml_fragment(
        &mut self,
        including: &Path,
        path: &Path,
        href: &str,
        chain: &mut Vec<PathBuf>,
    ) -> io::Result<String> {
        if chain.iter().any(|ancestor| ancestor == path) {
            return Err(invalid(
                including,
                format!("XInclude cycle through {}", path.display()),
            ));
        }
        if chain.len() >= 128 {
            return Err(invalid(
                including,
                "XIncludes are nested more than 128 levels",
            ));
        }
        let xml = if let Some(xml) = self.xml_fragments.get(path) {
            xml.clone()
        } else {
            let xml: Rc<str> = read_with_context(path, fs::read_to_string)?.into();
            self.xml_fragments.insert(path.to_path_buf(), xml.clone());
            xml
        };
        chain.push(path.to_path_buf());
        let xml = self.expand_xml_includes(path, xml, chain)?;
        chain.pop();
        let document = Document::parse(&xml).map_err(|error| invalid(path, error))?;
        let root = document.root_element();
        let fragment = &xml[root.range()];
        let end = opening_tag_end(fragment);
        let insert = end - usize::from(fragment.as_bytes()[end - 1] == b'/');
        let mut expanded = String::with_capacity(fragment.len());
        expanded.push_str(&fragment[..insert]);
        if root.tag_name().namespace().is_none()
            && !root
                .namespaces()
                .any(|namespace| namespace.name().is_none())
        {
            expanded.push_str(" xmlns=\"\"");
        }
        if root.attribute((XML_NS, "base")).is_none() {
            let base = if let Some((alias, relative)) = href.split_once(':') {
                format!(
                    "{}:{}",
                    alias.to_ascii_lowercase(),
                    relative.replace('\\', "/")
                )
            } else {
                let document_dir = chain[0].parent().expect("canonical document has a parent");
                relative_xml_path(document_dir, path)
            };
            expanded.push_str(" xml:base=\"");
            escape_xml(&base, true, &mut expanded);
            expanded.push('"');
        }
        expanded.push_str(&fragment[insert..]);
        Ok(expanded)
    }
}

fn relative_xml_path(directory: &Path, file: &Path) -> String {
    let mut directory = directory.components().peekable();
    let mut file = file.components().peekable();
    while directory.peek().is_some() && directory.peek() == file.peek() {
        directory.next();
        file.next();
    }
    let mut result = PathBuf::new();
    for _ in directory {
        result.push("..");
    }
    result.extend(file);
    result.to_string_lossy().replace('\\', "/")
}
