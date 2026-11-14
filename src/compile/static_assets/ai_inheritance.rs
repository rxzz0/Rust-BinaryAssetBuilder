use super::ai_personality::{CHILDREN, CRITERIA, FILTER, IN_TROUBLE, child_index, reference_name};
use super::ai_settings::MICRO_SCHEMA;
use super::{compile_asset, declaration_id, significant_children};
use crate::compile::{CompiledAsset, Compiler, SAGE_NS, escape_xml, invalid, is_xml_whitespace};
use crate::manifest::AssetId;
use roxmltree::{Document, Node};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PERSONALITY: &str = "AIPersonalityDefinition";
const MICRO: &str = "AIMicroManagerData";
const RETREAT: &str = "RetreatBaseCriteria";
const IGNORE: &str = "IgnoreTargets";
const XSD: &str = "http://www.w3.org/2001/XMLSchema";

impl Compiler<'_> {
    pub(in crate::compile) fn compile_inherited_ai(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let element = self.inherited_ai_xml(node, path, &mut Vec::new())?;
        let mut xml = String::new();
        element.write_xml(&mut xml, true);
        let document = Document::parse(&xml).map_err(|error| invalid(path, error))?;
        compile_asset(document.root_element(), path, source)
    }

    fn inherited_ai_xml(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        chain: &mut Vec<(PathBuf, AssetId)>,
    ) -> io::Result<Element> {
        let (_, id) = declaration_id(node, path)?;
        let Some(inherit_from) = node.attribute("inheritFrom") else {
            return Element::read(node, path);
        };
        if chain.len() >= 128
            || chain
                .iter()
                .any(|(file, asset)| file == path && *asset == id)
        {
            return Err(invalid(
                path,
                format!("cyclic or excessively deep AI inheritance from {inherit_from}"),
            ));
        }
        chain.push((path.to_path_buf(), id));
        let kind = node.tag_name().name();
        let base =
            self.with_xml_base(node, path, inherit_from, |parent, parent_path, included| {
                let mut base = self.inherited_ai_xml(parent, parent_path, chain)?;
                if included {
                    base.validate_for_inheritance(parent_path)?;
                }
                Ok(base)
            })?;
        chain.pop();

        // The SDK merges declarations in schema order and strips comments and
        // pipeline inheritance attributes when constructing the effective XML.
        let mut result = Element::new(kind);
        for mut input in [base, Element::read(node, path)?] {
            input.attributes.retain(|(name, _)| name != "inheritFrom");
            result.merge_attributes(input.attributes);
            for child in input.children {
                result.merge_child(child, path)?;
            }
        }
        Ok(result)
    }
}

struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Element>,
    text: String,
}

impl Element {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            attributes: Vec::new(),
            children: Vec::new(),
            text: String::new(),
        }
    }

    fn read(node: Node<'_, '_>, path: &Path) -> io::Result<Self> {
        let mut element = Self::new(node.tag_name().name());
        if node.tag_name().namespace() != Some(SAGE_NS) {
            return Err(invalid(
                path,
                format!("unexpected namespace on {}", element.name),
            ));
        }
        for attr in node.attributes() {
            if attr.namespace().is_some() {
                return Err(invalid(
                    path,
                    format!("unsupported AI inheritance attribute {}", attr.name()),
                ));
            }
            element
                .attributes
                .push((attr.name().to_owned(), attr.value().to_owned()));
        }
        for child in significant_children(node) {
            if child.is_element() {
                if element.name == PERSONALITY {
                    child_index(child, path)?;
                }
                element.children.push(Self::read(child, path)?);
            } else if child.is_text() && !is_xml_whitespace(child.text().unwrap_or_default()) {
                element.text.push_str(child.text().unwrap_or_default());
            }
        }
        if !element.text.is_empty()
            && (!element.children.is_empty()
                || matches!(element.name.as_str(), PERSONALITY | MICRO))
        {
            return Err(invalid(
                path,
                format!("unexpected text in {}", element.name),
            ));
        }
        Ok(element)
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn merge_attributes(&mut self, attributes: Vec<(String, String)>) {
        for (key, value) in attributes {
            if let Some((_, original)) = self.attributes.iter_mut().find(|(name, _)| *name == key) {
                *original = value;
            } else {
                self.attributes.push((key, value));
            }
        }
    }

    fn merge_child(&mut self, child: Self, path: &Path) -> io::Result<()> {
        let rank = |name: &str| match self.name.as_str() {
            PERSONALITY => CHILDREN.iter().position(|(tag, _, _)| *tag == name),
            MICRO => [RETREAT, IGNORE].iter().position(|tag| *tag == name),
            FILTER | IGNORE => ["IncludeThing", "ExcludeThing"]
                .iter()
                .position(|tag| *tag == name),
            _ => None,
        };
        let position = rank(&child.name)
            .ok_or_else(|| invalid(path, format!("unexpected {} in {}", child.name, self.name)))?;
        let singleton = matches!(
            child.name.as_str(),
            CRITERIA | FILTER | IN_TROUBLE | RETREAT | IGNORE
        );
        let id = child.attribute("id");
        if let Some(existing) = self.children.iter_mut().find(|item| {
            item.name == child.name && (singleton || id.is_some() && item.attribute("id") == id)
        }) {
            existing.merge_attributes(child.attributes);
            for nested in child.children {
                existing.merge_child(nested, path)?;
            }
        } else {
            let index = self
                .children
                .iter()
                .position(|item| rank(&item.name) > Some(position))
                .unwrap_or(self.children.len());
            self.children.insert(index, child);
        }
        Ok(())
    }

    fn validate_for_inheritance(&mut self, path: &Path) -> io::Result<()> {
        // Cross-file bases are already schema-validated when the SDK clones
        // them. Defaults become explicit, and strong references carry their
        // original occurrence index until the derived declaration is validated.
        static DEFAULTS: OnceLock<Defaults> = OnceLock::new();
        let defaults = DEFAULTS.get_or_init(schema_defaults);
        let add_defaults = |element: &mut Self| {
            if let Some((_, fields)) = defaults.iter().find(|(name, _)| *name == element.name) {
                for (name, value) in fields {
                    if element.attribute(name).is_none() {
                        element.attributes.push((name.clone(), value.clone()));
                    }
                }
            }
        };
        add_defaults(self);
        let mut ordinal = 0;
        for child in &mut self.children {
            add_defaults(child);
            let reference = match child.name.as_str() {
                "Side" => Some(&mut child.text),
                "PowerChoice" => child
                    .attributes
                    .iter_mut()
                    .find(|(name, _)| name == "PlayerPower")
                    .map(|(_, value)| value),
                "StrategicState" | "BudgetState" | "BuildState" => Some(
                    &mut child
                        .attributes
                        .iter_mut()
                        .find(|(name, _)| name == "State")
                        .ok_or_else(|| invalid(path, "missing AI state reference"))?
                        .1,
                ),
                _ => None,
            };
            if let Some(value) = reference.filter(|value| !value.trim().is_empty()) {
                *value = format!("{}\\{ordinal}", reference_name(value.trim()));
                ordinal += 1;
            }
        }
        Ok(())
    }

    fn write_xml(&self, xml: &mut String, root: bool) {
        xml.push('<');
        xml.push_str(&self.name);
        if root {
            xml.push_str(" xmlns=\"uri:ea.com:eala:asset\"");
        }
        for (name, value) in &self.attributes {
            xml.push(' ');
            xml.push_str(name);
            xml.push_str("=\"");
            escape_xml(value, true, xml);
            xml.push('"');
        }
        if self.children.is_empty() && self.text.is_empty() {
            xml.push_str(" />");
        } else {
            xml.push('>');
            escape_xml(&self.text, false, xml);
            for child in &self.children {
                child.write_xml(xml, false);
            }
            xml.push_str("</");
            xml.push_str(&self.name);
            xml.push('>');
        }
    }
}

type Defaults = Vec<(&'static str, Vec<(String, String)>)>;

fn schema_defaults() -> Defaults {
    const PERSONALITY_SCHEMA: &str =
        include_str!("../../../schemas/ra3/xsd/SkirmishAI/AssetTypeAIPersonalityDefinition.xsd");
    const BASE_CRITERIA: &str =
        include_str!("../../../schemas/ra3/xsd/SkirmishAI/AIBaseSelectionCriteria.xsd");
    const OBJECT_FILTER: &str = include_str!("../../../schemas/ra3/xsd/Includes/ObjectFilter.xsd");
    [
        (PERSONALITY, PERSONALITY, PERSONALITY_SCHEMA),
        ("UnitCap", "AIUnitCap", PERSONALITY_SCHEMA),
        (
            "ResourceMultiplierCheat",
            "AIResourceMultiplierCheat",
            PERSONALITY_SCHEMA,
        ),
        ("PowerChoice", "AIPlayerPowerChoice", PERSONALITY_SCHEMA),
        ("StrategicState", "AIStrategicState", PERSONALITY_SCHEMA),
        ("BudgetState", "AIBudgetState", PERSONALITY_SCHEMA),
        ("BuildState", "AIBuildState", PERSONALITY_SCHEMA),
        ("SpecificUnitCap", "AISpecificUnitCap", PERSONALITY_SCHEMA),
        (CRITERIA, "AIBaseSelectionCriteria", BASE_CRITERIA),
        (FILTER, "ObjectFilter", OBJECT_FILTER),
        (MICRO, MICRO, MICRO_SCHEMA),
        (RETREAT, "AIBaseSelectionCriteria", BASE_CRITERIA),
        (IGNORE, "ObjectFilter", OBJECT_FILTER),
    ]
    .into_iter()
    .map(|(element, kind, schema)| {
        let schema = Document::parse(schema).expect("bundled AI schema must be valid");
        let definition = schema
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name((XSD, "complexType")) && node.attribute("name") == Some(kind)
            })
            .expect("AI schema type must exist");
        let fields = definition
            .descendants()
            .filter(|node| node.has_tag_name((XSD, "attribute")))
            .filter_map(|node| Some((node.attribute("name")?, node.attribute("default")?)))
            .map(|(name, default)| (name.to_owned(), default.to_owned()))
            .collect::<Vec<_>>();
        (element, fields)
    })
    .collect()
}
