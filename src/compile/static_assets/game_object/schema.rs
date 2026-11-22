use super::super::asset_reference;
use crate::compile::{AssetReference, SAGE_NS, invalid};
use roxmltree::{Document, Node};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

mod validation;

const XSD: &str = "http://www.w3.org/2001/XMLSchema";
const SAGE_SCHEMA: &str = "uri:ea.com:eala:asset:schema";

#[derive(Default)]
pub(in crate::compile) struct Schema {
    types: HashMap<String, Type>,
    pub(in crate::compile) assets: Vec<(String, String)>,
}

#[derive(Default)]
struct Type {
    base: Option<String>,
    reference_target: Option<String>,
    fields: Vec<Field>,
    children: Vec<Field>,
    choice: bool,
    repeated_choice: bool,
    choice_min: usize,
    choice_max: usize,
    list_item: Option<String>,
    enumerators: Vec<String>,
    length: Option<usize>,
    min_length: Option<usize>,
    min_value: Option<f64>,
    max_value: Option<f64>,
}

pub(in crate::compile) struct Field {
    pub(in crate::compile) name: String,
    pub(in crate::compile) kind: String,
    pub(in crate::compile) default: Option<String>,
    pub(super) repeated: bool,
    minimum: usize,
    maximum: usize,
    explicitly_optional: bool,
    reference_target: Option<String>,
}

#[derive(Default)]
pub(in crate::compile) struct Shape<'a> {
    pub(in crate::compile) fields: Vec<&'a Field>,
    pub(in crate::compile) children: Vec<&'a Field>,
    pub(super) choice: bool,
    pub(super) repeated_choice: bool,
    choice_min: usize,
    choice_max: usize,
}

impl Shape<'_> {
    pub(super) fn optional_choice(&self) -> bool {
        self.choice_min == 0
    }
}

impl Schema {
    pub(in crate::compile) fn load(root: &Path) -> io::Result<Self> {
        let root = fs::canonicalize(root).map_err(|error| invalid(root, error))?;
        let mut pending = vec![root.join("CnC3Types.xsd")];
        let mut visited = HashSet::<PathBuf>::new();
        let mut schema = Self::default();
        while let Some(path) = pending.pop() {
            let path = fs::canonicalize(&path).map_err(|error| invalid(&path, error))?;
            if !path.starts_with(&root) {
                return Err(invalid(
                    &path,
                    "schema include leaves the SDK schema directory",
                ));
            }
            if !visited.insert(path.clone()) {
                continue;
            }
            let xml = fs::read_to_string(&path).map_err(|error| invalid(&path, error))?;
            let document = Document::parse(&xml).map_err(|error| invalid(&path, error))?;
            let directory = path.parent().expect("schema parent");
            for node in document.root_element().children().filter(Node::is_element) {
                if node.has_tag_name((XSD, "include")) {
                    let location = node
                        .attribute("schemaLocation")
                        .ok_or_else(|| invalid(&path, "missing schemaLocation"))?;
                    pending.push(directory.join(location));
                } else if node.has_tag_name((XSD, "element"))
                    && node.attribute("name") == Some("AssetDeclaration")
                {
                    for choice in node
                        .descendants()
                        .filter(|child| child.has_tag_name((XSD, "choice")))
                    {
                        schema.assets.extend(choice.children().filter_map(|child| {
                            if child.has_tag_name((XSD, "element")) {
                                Some((
                                    child.attribute("name")?.to_owned(),
                                    type_name(child.attribute("type")?),
                                ))
                            } else {
                                None
                            }
                        }));
                    }
                } else if is_type(node)
                    && let Some(name) = node.attribute("name")
                {
                    schema.read_type(name, node);
                }
            }
        }
        if !schema.types.contains_key("AudioEventOverridable")
            && schema.types.contains_key("AudioEvent")
        {
            // Uprising uses the same XML fields and payload layout as RA3 AudioEvent.
            schema.types.insert(
                "AudioEventOverridable".into(),
                Type {
                    base: Some("AudioEvent".into()),
                    ..Type::default()
                },
            );
        }
        Ok(schema)
    }

    fn read_type(&mut self, name: &str, node: Node<'_, '_>) {
        let mut kind = Type {
            reference_target: node.attribute((SAGE_SCHEMA, "refType")).map(str::to_owned),
            ..Type::default()
        };
        let content = node.children().find(|child| {
            child.has_tag_name((XSD, "complexContent"))
                || child.has_tag_name((XSD, "simpleContent"))
        });
        let body = content
            .and_then(|content| {
                content.children().find(|child| {
                    child.has_tag_name((XSD, "extension"))
                        || child.has_tag_name((XSD, "restriction"))
                })
            })
            .unwrap_or(node);
        kind.base = body.attribute("base").map(type_name);
        for child in body.children().filter(Node::is_element) {
            match (child.tag_name().namespace(), child.tag_name().name()) {
                (Some(XSD), "attribute") => {
                    if let Some(field) = self.field(name, "@", child) {
                        kind.fields.push(field);
                    }
                }
                (Some(XSD), "sequence" | "choice" | "all") => {
                    kind.choice = child.has_tag_name((XSD, "choice"));
                    kind.repeated_choice = kind.choice && repeated(child);
                    if kind.choice {
                        kind.choice_min = occurs(child, "minOccurs");
                        kind.choice_max = occurs(child, "maxOccurs");
                    }
                    for entry in child
                        .children()
                        .filter(|entry| entry.has_tag_name((XSD, "element")))
                    {
                        if let Some(field) = self.field(name, "/", entry) {
                            kind.children.push(field);
                        }
                    }
                    if kind.choice && kind.children.iter().any(|field| field.minimum == 0) {
                        kind.choice_min = 0;
                    }
                }
                (Some(XSD), "restriction") => {
                    kind.base = child.attribute("base").map(type_name);
                    let facet = |name| {
                        child
                            .children()
                            .find(|facet| facet.has_tag_name((XSD, name)))
                            .and_then(|facet| facet.attribute("value"))
                    };
                    kind.length = facet("length").and_then(|value| value.parse().ok());
                    kind.min_length = facet("minLength").and_then(|value| value.parse().ok());
                    kind.min_value = facet("minInclusive").and_then(|value| value.parse().ok());
                    kind.max_value = facet("maxInclusive").and_then(|value| value.parse().ok());
                    kind.enumerators = child
                        .children()
                        .filter(|facet| facet.has_tag_name((XSD, "enumeration")))
                        .filter_map(|facet| facet.attribute("value").map(str::to_owned))
                        .collect();
                }
                (Some(XSD), "list") => {
                    kind.list_item = child.attribute("itemType").map(type_name);
                    if let Some(item) = child.children().find(|child| is_type(*child)) {
                        let item_name = format!("{name}/item");
                        self.read_type(&item_name, item);
                        kind.list_item = Some(item_name);
                    }
                }
                _ => {}
            }
        }
        self.types.insert(name.to_owned(), kind);
    }

    fn field(&mut self, parent: &str, separator: &str, node: Node<'_, '_>) -> Option<Field> {
        let name = node.attribute("name").or_else(|| node.attribute("ref"))?;
        let kind = if let Some(name) = node.attribute("type") {
            type_name(name)
        } else if let Some(inline) = node.children().find(|child| is_type(*child)) {
            let kind = format!("{parent}{separator}{name}");
            self.read_type(&kind, inline);
            kind
        } else {
            "xs:string".to_owned()
        };
        Some(Field {
            name: name.to_owned(),
            kind,
            default: node.attribute("default").map(str::to_owned),
            repeated: repeated(node),
            minimum: if node.has_tag_name((XSD, "attribute")) {
                usize::from(node.attribute("use") == Some("required"))
            } else {
                occurs(node, "minOccurs")
            },
            maximum: occurs(node, "maxOccurs"),
            explicitly_optional: node.attribute("use") == Some("optional")
                || node.attribute("minOccurs") == Some("0"),
            reference_target: node.attribute((SAGE_SCHEMA, "refType")).map(str::to_owned),
        })
    }

    pub(in crate::compile) fn shape(&self, kind: &str, path: &Path) -> io::Result<Shape<'_>> {
        let mut lineage = Vec::new();
        let mut name = kind;
        while !name.starts_with("xs:") {
            if lineage.len() >= 64 {
                return Err(invalid(
                    path,
                    format!("cyclic or excessively deep schema type {kind}"),
                ));
            }
            let entry = self
                .types
                .get(name)
                .ok_or_else(|| invalid(path, format!("missing SDK schema type {name}")))?;
            lineage.push(entry);
            let Some(base) = &entry.base else { break };
            name = base;
        }
        let mut shape = Shape::default();
        // The SDK validator adds defaults from the derived type before
        // defaults inherited from its base types.
        for entry in &lineage {
            shape.fields.extend(&entry.fields);
        }
        for entry in lineage.into_iter().rev() {
            if !entry.children.is_empty() {
                shape.children.extend(&entry.children);
                shape.choice = entry.choice;
                shape.repeated_choice = entry.repeated_choice;
                shape.choice_min = entry.choice_min;
                shape.choice_max = if entry.children.iter().any(|field| field.repeated) {
                    usize::MAX
                } else {
                    entry.choice_max
                };
            }
        }
        Ok(shape)
    }

    pub(super) fn is_enum_list(&self, kind: &str) -> bool {
        self.types
            .get(kind)
            .and_then(|kind| kind.list_item.as_deref())
            .and_then(|item| self.types.get(item))
            .is_some_and(|item| !item.enumerators.is_empty())
    }

    pub(super) fn is_reference(&self, kind: &str) -> bool {
        let mut kind = kind;
        for _ in 0..64 {
            if kind == "AssetReference" {
                return true;
            }
            if kind == "WeakReference" {
                return false;
            }
            let Some(base) = self.types.get(kind).and_then(|kind| kind.base.as_deref()) else {
                return false;
            };
            kind = base;
        }
        false
    }

    pub(in crate::compile) fn weak_references(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        map: bool,
    ) -> io::Result<Vec<AssetReference>> {
        let mut references = Vec::new();
        self.find_weak_references(node, node.tag_name().name(), path, map, &mut references)?;
        Ok(references)
    }

    fn find_weak_references(
        &self,
        node: Node<'_, '_>,
        kind: &str,
        path: &Path,
        map: bool,
        references: &mut Vec<AssetReference>,
    ) -> io::Result<()> {
        if !self.types.contains_key(kind) {
            return Ok(());
        }
        let shape = self.shape(kind, path)?;
        for attribute in node.attributes().filter(|attr| attr.namespace().is_none()) {
            if let Some(field) = shape
                .fields
                .iter()
                .find(|field| field.name == attribute.name())
                && let Some(target) = self.weak_reference_target(field, map)
            {
                add_weak_reference(attribute.value(), target, path, map, references)?;
            }
        }
        for child in node
            .children()
            .filter(|child| child.tag_name().namespace() == Some(SAGE_NS))
        {
            let Some(field) = shape
                .children
                .iter()
                .find(|field| field.name == child.tag_name().name())
            else {
                continue;
            };
            if let Some(target) = self.weak_reference_target(field, map) {
                add_weak_reference(
                    child.text().unwrap_or_default(),
                    target,
                    path,
                    map,
                    references,
                )?;
            } else {
                self.find_weak_references(child, &field.kind, path, map, references)?;
            }
        }
        Ok(())
    }

    fn weak_reference_target<'a>(&'a self, field: &'a Field, map: bool) -> Option<&'a str> {
        let mut kind = field.kind.as_str();
        let mut target = field.reference_target.as_deref();
        for _ in 0..64 {
            if kind == "WeakReference" {
                return target.filter(|target| {
                    map || matches!(
                        *target,
                        "GameObject" | "LogicCommandSet" | "PackedTextureImage" | "UpgradeTemplate"
                    )
                });
            }
            let entry = self.types.get(kind)?;
            target = target.or(entry.reference_target.as_deref());
            kind = entry.base.as_deref()?;
        }
        None
    }
}

fn add_weak_reference(
    value: &str,
    target: &str,
    path: &Path,
    map: bool,
    references: &mut Vec<AssetReference>,
) -> io::Result<()> {
    let value = value.trim();
    if !value.is_empty() {
        let kind = value.split_once(':').map_or(target, |(kind, _)| kind);
        match asset_reference(value, kind, path) {
            Ok(reference) => references.push(reference.into()),
            Err(error) if map => return Err(error),
            Err(_) => {}
        }
    }
    Ok(())
}

fn is_type(node: Node<'_, '_>) -> bool {
    node.has_tag_name((XSD, "complexType")) || node.has_tag_name((XSD, "simpleType"))
}

fn occurs(node: Node<'_, '_>, attribute: &str) -> usize {
    match node.attribute(attribute) {
        Some("unbounded") => usize::MAX,
        Some(value) => value.parse().unwrap_or(1),
        None => 1,
    }
}

fn repeated(node: Node<'_, '_>) -> bool {
    node.attribute("maxOccurs")
        .is_some_and(|count| !matches!(count, "0" | "1"))
}

fn type_name(name: &str) -> String {
    match name.split_once(':') {
        Some((prefix, local)) if prefix != "xs" => local.to_owned(),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::static_assets::game_object::{Element, Join};

    #[test]
    fn merges_game_object_modules_and_flags_using_schema_order() {
        let path = Path::new("game-object.xml");
        let xsd = Document::parse(
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
                <xs:simpleType name="AssetReference"><xs:restriction base="xs:string"/></xs:simpleType>
                <xs:simpleType name="Flag"><xs:restriction base="xs:string">
                    <xs:enumeration value="STRUCTURE"/><xs:enumeration value="SELECTABLE"/>
                </xs:restriction></xs:simpleType>
                <xs:simpleType name="Flags"><xs:list itemType="Flag"/></xs:simpleType>
                <xs:complexType name="ModuleBase"><xs:attribute name="Enabled" type="xs:boolean" default="true"/></xs:complexType>
                <xs:complexType name="Module"><xs:complexContent><xs:extension base="ModuleBase">
                    <xs:attribute name="Radius" type="xs:float" default="5"/>
                    <xs:attribute name="Weapon" type="AssetReference"/>
                </xs:extension></xs:complexContent></xs:complexType>
                <xs:complexType name="Modules"><xs:choice maxOccurs="unbounded">
                    <xs:element name="Helper" type="Module"/>
                    <xs:element name="Replacement" type="Module"/>
                </xs:choice></xs:complexType>
                <xs:complexType name="GameObject"><xs:sequence>
                    <xs:element name="DisplayName" type="xs:string"/>
                    <xs:element name="Behaviors" type="Modules"/>
                </xs:sequence><xs:attribute name="KindOf" type="Flags"/></xs:complexType>
            </xs:schema>"#,
        ).unwrap();
        let mut schema = Schema::default();
        for item in xsd.root_element().children().filter(Node::is_element) {
            schema.read_type(item.attribute("name").unwrap(), item);
        }
        let base = Document::parse(
            r#"<GameObject xmlns="uri:ea.com:eala:asset" id="Base" KindOf="STRUCTURE">
                <Behaviors>
                    <Helper id="Keep" Weapon="First"/>
                    <Helper id="Replace" Radius="100"/>
                    <Helper id="Remove"/>
                </Behaviors>
                <DisplayName>Base name</DisplayName>
            </GameObject>"#,
        )
        .unwrap();
        let derived = Document::parse(
            r#"<GameObject xmlns="uri:ea.com:eala:asset" xmlns:xai="uri:ea.com:eala:asset:instance"
                id="Derived" inheritFrom="Base" KindOf="-STRUCTURE +SELECTABLE">
                <Behaviors>
                    <Replacement id="Replace" xai:joinAction="Replace"/>
                    <Helper id="Remove" xai:joinAction="Remove"/>
                </Behaviors>
                <DisplayName xai:joinAction="Replace">Derived name</DisplayName>
            </GameObject>"#,
        )
        .unwrap();
        let mut base = Element::read(base.root_element(), path).unwrap();
        base.materialize(&schema, "GameObject", &mut 0, path)
            .unwrap();
        let mut result = Element::new("GameObject");
        for node in [base, Element::read(derived.root_element(), path).unwrap()] {
            result
                .merge(node, &schema, "GameObject", Join::Append, path)
                .unwrap();
        }
        let mut xml = String::new();
        result.write_xml(&mut xml, true);
        assert_eq!(
            xml,
            r#"<GameObject id="Derived" KindOf=" SELECTABLE" xmlns="uri:ea.com:eala:asset"><DisplayName>Derived name</DisplayName><Behaviors><Helper id="Keep" Weapon="First\0" Radius="5" Enabled="true" /><Replacement id="Replace" /></Behaviors></GameObject>"#,
        );
    }
}
