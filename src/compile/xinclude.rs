use super::{
    Compiler, SAGE_NS, XML_NS, escape_xml, invalid, opening_tag_end, read_with_context, required,
    write_namespace_attributes,
};
use roxmltree::{Document, Node};
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const XI_NS: &str = "http://www.w3.org/2001/XInclude";
// Carry inclusion provenance through reparsing without changing the asset hash.
const INCLUDED_SCRIPT: &str = "bab-included-script";

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
                    attr.namespace().is_some()
                        || !matches!(attr.name(), "href" | "parse" | "xpointer")
                })
                || !matches!(include.attribute("parse"), None | Some("xml"))
                || include.children().any(|child| {
                    child.is_element()
                        || child.is_text()
                            && child.text().is_some_and(|text| !text.trim().is_empty())
                })
            {
                return Err(invalid(path, "expected an XML xi:include"));
            }
            let href = required(include, "href", path)?;
            if href.contains('#') {
                return Err(invalid(path, "XInclude fragment selectors are unsupported"));
            }
            let target = self.resolve(path, href)?;
            let range = include.range();
            expanded.push_str(&xml[copied..range.start]);
            let fragment = self.xml_fragment(path, &target, href, include, chain)?;
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
        include: Node<'_, '_>,
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
        let mut expanded = String::new();
        if let Some(pointer) = include.attribute("xpointer") {
            if !matches_root(pointer, root)
                .ok_or_else(|| invalid(including, format!("unsupported XPointer {pointer:?}")))?
            {
                return Err(invalid(
                    including,
                    format!("XPointer {pointer:?} does not match {}", path.display()),
                ));
            }
            let mut children = root.children().filter(Node::is_element).peekable();
            if children.peek().is_none() {
                return Err(invalid(
                    including,
                    format!(
                        "XPointer {pointer:?} selected no elements in {}",
                        path.display()
                    ),
                ));
            }
            for child in children {
                write_fragment(child, include, &base, &mut expanded);
            }
        } else {
            write_fragment(root, include, &base, &mut expanded);
        }
        Ok(expanded)
    }
}

fn matches_root(mut pointer: &str, root: Node<'_, '_>) -> Option<bool> {
    pointer = pointer.trim();
    let mut binding = None;
    if let Some(declaration) = pointer.strip_prefix("xmlns(") {
        let (declaration, rest) = declaration.split_once(')')?;
        let (prefix, uri) = declaration.split_once('=')?;
        binding = Some((prefix.trim(), uri.trim()));
        pointer = rest.trim_start();
    }
    let (name, axis) = pointer
        .strip_prefix("xpointer(/")?
        .strip_suffix(')')?
        .split_once('/')?;
    if !matches!(axis, "child::*" | "*") {
        return None;
    }
    let (name, namespace) = match name.split_once(':') {
        Some((prefix, name)) => {
            let (bound_prefix, uri) = binding?;
            if prefix != bound_prefix {
                return None;
            }
            (name, Some(uri))
        }
        None => (name, None),
    };
    Some(root.tag_name().name() == name && root.tag_name().namespace() == namespace)
}

fn write_fragment(node: Node<'_, '_>, include: Node<'_, '_>, base: &str, expanded: &mut String) {
    let input = node.document().input_text();
    let range = node.range();
    let fragment = &input[range.clone()];
    let end = opening_tag_end(fragment);
    let insert = end - usize::from(fragment.as_bytes()[end - 1] == b'/');
    expanded.push_str(&fragment[..insert]);
    if !node
        .namespaces()
        .any(|namespace| namespace.name().is_none())
        && (node.parent_element().is_none()
            || include
                .parent_element()
                .is_some_and(|parent| parent.lookup_namespace_uri(None).is_some()))
    {
        expanded.push_str(" xmlns=\"\"");
    }
    if node.parent_element().is_some() {
        let mut missing_namespaces = node
            .namespaces()
            .filter(|namespace| {
                include
                    .parent_element()
                    .and_then(|parent| parent.lookup_namespace_uri(namespace.name()))
                    != Some(namespace.uri())
            })
            .peekable();
        if missing_namespaces.peek().is_some() {
            let name_end = fragment
                .find(|character: char| character.is_whitespace() || matches!(character, '/' | '>'))
                .expect("parsed XML element name");
            let mut local = String::new();
            let mut cursor = range.start + name_end;
            for attribute in node.attributes() {
                write_namespace_attributes(
                    node,
                    None,
                    &input[cursor..attribute.range().start],
                    &mut local,
                );
                cursor = attribute.range().end;
            }
            write_namespace_attributes(
                node,
                None,
                &input[cursor..range.start + insert],
                &mut local,
            );
            for namespace in missing_namespaces {
                let name = namespace.name().map_or_else(
                    || " xmlns=\"".to_owned(),
                    |prefix| format!(" xmlns:{prefix}=\""),
                );
                if !local.contains(&name) {
                    expanded.push_str(&name);
                    escape_xml(namespace.uri(), true, expanded);
                    expanded.push('"');
                }
            }
        }
    }
    if node.attribute((XML_NS, "base")).is_none() {
        expanded.push_str(" xml:base=\"");
        escape_xml(base, true, expanded);
        expanded.push('"');
    }
    let mut copied = range.start + insert;
    for script in node.descendants().filter(|child| {
        child.has_tag_name((SAGE_NS, "Script"))
            && (child
                .parent_element()
                .is_some_and(|parent| parent.has_tag_name((SAGE_NS, "AnimationState")))
                || (*child == node
                    && include
                        .parent_element()
                        .is_some_and(|parent| parent.has_tag_name((SAGE_NS, "AnimationState")))))
    }) {
        let source = &input[script.range()];
        let tag_end = opening_tag_end(source) + 1;
        let contents = &source[tag_end..];
        if (contents.contains('\r') || contents.contains("<![CDATA["))
            && !has_included_script_marker(script)
        {
            let end = script.range().start + tag_end;
            expanded.push_str(&input[copied..end]);
            expanded.push_str("<?");
            expanded.push_str(INCLUDED_SCRIPT);
            expanded.push_str("?>");
            copied = end;
        }
    }
    expanded.push_str(&input[copied..range.end]);
}

pub(super) fn has_included_script_marker(script: Node<'_, '_>) -> bool {
    script.has_tag_name((SAGE_NS, "Script"))
        && script
            .first_child()
            .and_then(|child| child.pi())
            .is_some_and(|pi| pi.target == INCLUDED_SCRIPT && pi.value.is_none())
}

pub(super) fn script_text<'a>(node: Node<'a, '_>) -> impl Iterator<Item = Cow<'a, str>> {
    let included = has_included_script_marker(node);
    node.children().filter(Node::is_text).map(move |child| {
        if !included {
            return Cow::Borrowed(child.text().expect("XML text node"));
        }
        // EA's XInclude reader keeps physical CRs in the script value.
        // Preserve the original text and CDATA in the XML used for hashing.
        let input = node.document().input_text();
        let end = child.next_sibling().map_or_else(
            || node.range().start + input[node.range()].rfind("</").unwrap(),
            |next| next.range().start,
        );
        let source = &input[child.range().start..end];
        if !source.contains('\r') {
            return Cow::Borrowed(child.text().expect("XML text node"));
        }
        let mut escaped = String::from("<text>");
        let mut rest = source;
        while let Some((text, cdata)) = rest.split_once("<![CDATA[") {
            escaped.push_str(&text.replace('\r', "&#13;"));
            let (content, after) = cdata.split_once("]]>").expect("parsed XML CDATA");
            escaped.push_str("<![CDATA[");
            escaped.push_str(&content.replace('\r', "]]>&#13;<![CDATA["));
            escaped.push_str("]]>");
            rest = after;
        }
        escaped.push_str(&rest.replace('\r', "&#13;"));
        escaped.push_str("</text>");
        let document = Document::parse(&escaped).expect("script text from parsed XML");
        Cow::Owned(
            document
                .root_element()
                .text()
                .unwrap_or_default()
                .to_owned(),
        )
    })
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
