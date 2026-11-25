mod active_body;
mod ai_update;
mod animation;
mod assisted_placement_info;
mod attack_assist;
mod auto_deposit;
mod behaviors_basic;
pub(super) use behaviors_basic::optional_vector;
mod bezier_projectile;
mod building_destruction;
mod capture_upgrade;
pub(super) mod children;
mod client_behaviors;
mod client_updates;
mod create_object_die;
mod draws;
mod dynamics;
mod eject_passengers;
mod enable_dynamics_collide;
pub(in crate::compile) use draws::SCRIPTED_FLAGS;
mod animal_ai;
mod attach;
mod beam_special_ability;
mod bombard_area_update;
mod circular_damage;
mod crate_collide;
mod destroy_objects;
mod encoder;
mod energy_disabling_behavior;
mod engineer;
mod fields;
mod fx_list_behavior;
mod garrison;
mod grant_upgrade_create;
mod height_die_update;
mod infiltrator;
mod invalid_targets_death_update;
mod invisibility_broadcast;
mod laser_draw;
mod laser_scatter;
mod laser_state;
mod line_segment_damage;
mod linear_damage;
pub(super) mod material;
mod parabolic_curve_update;
mod projectile_update;
mod radar_update;
mod selective_visibility;
mod set_locomotor_speed;
mod shield_sphere_update;
mod strafe_area_update;
mod unit_vision_behavior;
mod weapon_scatter_effect;
pub(super) use material::encode_render_material;
mod grappling_hook;
mod ore;
mod parking;
pub(super) use parking::encode_filter as encode_object_filter;
pub(super) use parking::encode_inline_filter as encode_inline_object_filter;
mod passive_area;
mod physics;
mod production;
mod recall;
mod repair;
mod replace_self;
mod schema;
mod secondary_damage;
mod secondary_listener;
pub(super) use shadow::encode_settings_shadow;
mod aim_weapon;
mod dominate_enemy;
mod hit_reaction;
mod lose_health_update;
mod ocl_monitor_update;
mod shadow;
mod slaved;
mod slow_death;
mod spawn;
mod special_disguise;
pub(super) mod special_power;
mod squeeze_fling;
mod strafe_area_target_chooser;
mod stream_draw;
mod structures;
mod sub_structure_storage;
mod subobjects;
mod tech_upgrade;
mod teleport_stored;
mod transform_upgrades;

pub(super) use behaviors_basic::{encode_die_mux, weapon_slot};
pub(super) use client_behaviors::audio_reference;
pub(super) use draws::{model_condition_index, write_model_conditions};
pub(super) use encoder::compile;
pub(super) use parking::encode_inline_filter;
pub(super) use passive_area::category_flags;
pub(super) use secondary_damage::write_status_flags;
pub(super) use spawn::{id_list as weak_id_list, weak_reference as weak_xml_reference};

pub(super) fn object_filter_asset(node: Node<'_, '_>, path: &Path) -> io::Result<super::Marshaled> {
    super::validate_attributes(node, &["id"], path)?;
    let mut children = super::significant_children(node);
    let filter = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "Filter")))
        .ok_or_else(|| invalid(path, "ObjectFilterAsset requires Filter"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ObjectFilterAsset child"));
    }
    let mut data = super::binary::BinaryData {
        payload: vec![0; 0x74],
        relocations: Vec::new(),
        path,
    };
    let mut reference_types = Vec::new();
    parking::encode_inline_filter(filter, 4, &mut data, &mut reference_types)?;
    if !data.relocations.is_empty() {
        data.relocations.push(u32::MAX);
    }
    Ok(super::Marshaled {
        payload: data.payload,
        relocations: data.relocations,
        reference_types,
        ..super::Marshaled::default()
    })
}

use super::{asset_reference, declaration_id};
use crate::compile::{
    CompiledAsset, Compiler, SAGE_NS, XML_NS, escape_xml, invalid, is_xml_whitespace,
};
use crate::manifest::AssetId;
use roxmltree::{Document, Node};
pub(in crate::compile) use schema::Schema;
use schema::Shape;
use std::io;
use std::path::{Path, PathBuf};

const XAI_NS: &str = "uri:ea.com:eala:asset:instance";

impl Compiler<'_> {
    pub(in crate::compile) fn compile_schema_inherited_asset(
        &mut self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        if self.game_object_schema.is_none() {
            self.game_object_schema = Some(Schema::load(&self.root.join("Schemas/xsd"))?);
        }
        let schema = self
            .game_object_schema
            .as_ref()
            .expect("loaded GameObject schema");
        let result = self.schema_inherited_xml(node, path, schema, &mut Vec::new())?;
        let mut xml = String::new();
        result.write_xml(&mut xml, true);
        let document = Document::parse(&xml).map_err(|error| invalid(path, error))?;
        self.compile_effective_asset(document.root_element(), path, source)
    }

    fn schema_inherited_xml(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        schema: &Schema,
        chain: &mut Vec<(PathBuf, AssetId)>,
    ) -> io::Result<Element> {
        let mut overlay = Element::read(node, path)?;
        if self.reference_mode.validates_schema() && overlay.name == "GameObject" {
            let asset = format!("GameObject:{}", node.attribute("id").unwrap_or_default());
            overlay.remove_unknown_optional_modules(schema, "GameObject", &asset, path)?;
        }
        let Some(inherit_from) = node.attribute("inheritFrom") else {
            return Ok(overlay);
        };
        let (_, id) = declaration_id(node, path)?;
        if chain.len() >= 128
            || chain
                .iter()
                .any(|(file, asset)| file == path && *asset == id)
        {
            return Err(invalid(
                path,
                format!(
                    "cyclic or excessively deep {} inheritance from {inherit_from}",
                    node.tag_name().name()
                ),
            ));
        }
        if overlay.action == Some(Join::Remove) {
            return Err(invalid(
                path,
                format!("cannot remove a top-level {}", node.tag_name().name()),
            ));
        }
        chain.push((path.to_path_buf(), id));
        let kind = node.tag_name().name();
        let mut result = Element::new(kind);
        self.with_xml_base(node, path, inherit_from, |parent, parent_path, included| {
            if overlay.action != Some(Join::Replace) {
                let mut base = self.schema_inherited_xml(parent, parent_path, schema, chain)?;
                if included {
                    base.materialize(schema, kind, &mut 0, parent_path)?;
                }
                result.merge(base, schema, kind, Join::Append, path)?;
            }
            Ok(())
        })?;
        chain.pop();
        let action = overlay.action.unwrap_or(Join::Append);
        result.merge(overlay, schema, kind, action, path)?;
        Ok(result)
    }

    pub(super) fn with_xml_base<T>(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        inherit_from: &str,
        use_base: impl FnOnce(Node<'_, '_>, &Path, bool) -> io::Result<T>,
    ) -> io::Result<T> {
        let kind = node.tag_name().name();
        let parent_id = asset_reference(inherit_from, kind, path)?.id();
        let alternate = if kind == "AudioEventOverridable" {
            let name = inherit_from
                .trim()
                .rsplit_once(':')
                .map_or(inherit_from.trim(), |(_, name)| name);
            // The RA3 SDK has the original forms of these Uprising templates.
            match name {
                "BaseSoundEffectOverridable" => Some("BaseSoundEffect"),
                "BaseUnitResponseOverridable" => Some("BaseUnitResponse"),
                _ => None,
            }
            .map(|name| asset_reference(name, "AudioEvent", path).map(|reference| reference.id()))
            .transpose()?
        } else {
            None
        };
        let parents = || std::iter::once(parent_id).chain(alternate);
        let declaration = node.document().root_element();
        if let Some(parent) = declaration.children().find(|candidate| {
            *candidate != node
                && candidate.has_tag_name((SAGE_NS, kind))
                && declaration_id(*candidate, path).is_ok_and(|(_, id)| id == parent_id)
        }) {
            return use_base(parent, path, false);
        }
        let mut instance_paths = Vec::new();
        for include in declaration
            .children()
            .filter(|item| item.has_tag_name((SAGE_NS, "Includes")))
            .flat_map(|includes| includes.children())
            .filter(|item| {
                item.has_tag_name((SAGE_NS, "Include"))
                    && item.attribute("type") == Some("instance")
            })
        {
            if let Some(source) = include.attribute("source") {
                instance_paths.push(self.resolve(path, source)?);
            }
        }
        let candidate = if self.reference_mode == crate::compile::ReferenceMode::Map {
            // A document can expose declarations through nested all-includes.
            // Instance includes of those children are only visible to the child.
            let mut pending = instance_paths;
            pending.extend(
                self.map_all_includes
                    .get(path)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            let mut reachable = std::collections::HashSet::new();
            while let Some(candidate) = pending.pop() {
                if reachable.insert(candidate.clone()) {
                    pending.extend(
                        self.map_all_includes
                            .get(&candidate)
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                }
            }
            parents().find_map(|parent_id| {
                Self::map_candidate(self.tentative.get(&parent_id).into_iter().flatten().filter(
                    |candidate| candidate.path != path && reachable.contains(&candidate.path),
                ))
            })
        } else {
            parents()
                .find_map(|parent_id| {
                    let mut candidates = self
                        .tentative
                        .get(&parent_id)
                        .into_iter()
                        .flatten()
                        .filter(|candidate| instance_paths.contains(&candidate.path));
                    candidates.next().map(|first| (first, candidates.next()))
                })
                .and_then(|(candidate, other)| other.is_none().then_some(candidate))
        }
        .ok_or_else(|| {
            invalid(
                path,
                format!("expected one included base for {kind} {inherit_from}"),
            )
        })?;
        let document =
            Document::parse(&candidate.xml).map_err(|error| invalid(&candidate.path, error))?;
        let parent = document
            .get_node(candidate.node)
            .ok_or_else(|| invalid(&candidate.path, "missing inherited asset declaration"))?;
        use_base(parent, &candidate.path, true)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Append,
    Replace,
    Remove,
}

struct Element {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Element>,
    text: String,
    action: Option<Join>,
    top: bool,
}

impl Element {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            attributes: Vec::new(),
            children: Vec::new(),
            text: String::new(),
            action: None,
            top: false,
        }
    }

    fn read(node: Node<'_, '_>, path: &Path) -> io::Result<Self> {
        let name = node.tag_name().name();
        if node.tag_name().namespace() != Some(SAGE_NS) {
            return Err(invalid(
                path,
                format!("unexpected namespace on GameObject element {name}"),
            ));
        }
        let mut element = Self::new(name);
        for attr in node.attributes() {
            let value = attr.value();
            match (attr.namespace(), attr.name()) {
                (None, "inheritFrom" | "TypeId") => {}
                (None, name) => element.attributes.push((name.to_owned(), value.to_owned())),
                (Some(XML_NS), "base") => element
                    .attributes
                    .push(("xml:base".to_owned(), value.to_owned())),
                (Some(XAI_NS), "joinAction") => {
                    element.action = Some(match value.to_ascii_lowercase().as_str() {
                        "append" => Join::Append,
                        "replace" | "overwrite" | "override" => Join::Replace,
                        "remove" => Join::Remove,
                        _ => {
                            return Err(invalid(
                                path,
                                format!("invalid {name} joinAction {value:?}"),
                            ));
                        }
                    });
                }
                (Some(XAI_NS), "insertPosition") => {
                    element.top = match value.to_ascii_lowercase().as_str() {
                        "top" => true,
                        "bottom" => false,
                        _ => {
                            return Err(invalid(
                                path,
                                format!("invalid {name} insertPosition {value:?}"),
                            ));
                        }
                    };
                }
                _ => {
                    return Err(invalid(
                        path,
                        format!("unsupported {name} attribute {}", attr.name()),
                    ));
                }
            }
        }
        for child in node.children() {
            if child.is_element() {
                element.children.push(Self::read(child, path)?);
            } else if let Some(text) = child
                .text()
                .filter(|text| child.is_text() && !is_xml_whitespace(text))
            {
                element.text.push_str(text);
            }
        }
        Ok(element)
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn remove_unknown_optional_modules(
        &mut self,
        schema: &Schema,
        kind: &str,
        asset: &str,
        path: &Path,
    ) -> io::Result<()> {
        let shape = schema.shape(kind, path)?;
        if shape.is_module_choice() && shape.optional_choice() {
            self.children.retain(|child| {
                if shape.children.iter().any(|field| field.name == child.name) {
                    return true;
                }
                crate::compile::warning(format_args!(
                    "Unexpected module '{}' in '{}' for '{asset}' in '{}'",
                    child.name,
                    self.name,
                    path.display()
                ));
                false
            });
        }
        for child in &mut self.children {
            if let Some(field) = shape.children.iter().find(|field| field.name == child.name) {
                child.remove_unknown_optional_modules(schema, &field.kind, asset, path)?;
            }
        }
        Ok(())
    }

    fn merge(
        &mut self,
        source: Self,
        schema: &Schema,
        kind: &str,
        action: Join,
        path: &Path,
    ) -> io::Result<()> {
        let shape = schema.shape(kind, path)?;
        self.text.push_str(&source.text);
        for child in source.children {
            self.merge_child(child, schema, &shape, action, path)?;
        }
        for (name, value) in source.attributes {
            let index = self.attributes.iter().position(|(key, _)| *key == name);
            let value = if (value.contains('+') || value.contains('-'))
                && shape
                    .fields
                    .iter()
                    .find(|field| field.name == name)
                    .is_some_and(|field| schema.is_enum_list(&field.kind))
            {
                let previous = index.map_or("", |index| self.attributes[index].1.as_str());
                update_flags(previous, &value, &name, path)?
            } else {
                value
            };
            if let Some(index) = index {
                self.attributes[index].1 = value;
            } else {
                self.attributes.push((name, value));
            }
        }
        Ok(())
    }

    fn merge_child(
        &mut self,
        child: Self,
        schema: &Schema,
        shape: &Shape<'_>,
        action: Join,
        path: &Path,
    ) -> io::Result<()> {
        let position = shape
            .children
            .iter()
            .position(|field| field.name == child.name)
            .ok_or_else(|| invalid(path, format!("unexpected {} in {}", child.name, self.name)))?;
        let field = shape.children[position];
        let action = child.action.unwrap_or(action);
        let matched = if shape.repeated_choice || !shape.choice && field.repeated {
            child.attribute("id").and_then(|id| {
                self.children
                    .iter()
                    .position(|item| item.attribute("id") == Some(id))
            })
        } else if shape.choice && self.children.len() == 1 {
            Some(0)
        } else {
            self.children
                .iter()
                .position(|item| item.name == child.name)
        };
        if let Some(index) = matched {
            if action == Join::Append && self.children[index].name == child.name {
                return self.children[index].merge(child, schema, &field.kind, action, path);
            }
            self.children.remove(index);
        }
        if action == Join::Remove {
            return Ok(());
        }
        let mut element = Self::new(&child.name);
        let index = if shape.choice {
            if child.top { 0 } else { self.children.len() }
        } else if child.top && self.children.iter().any(|item| item.name == child.name) {
            0
        } else {
            self.children
                .iter()
                .position(|item| {
                    shape
                        .children
                        .iter()
                        .position(|field| field.name == item.name)
                        .is_some_and(|rank| rank > position)
                })
                .unwrap_or(self.children.len())
        };
        element.merge(child, schema, &field.kind, action, path)?;
        self.children.insert(index, element);
        Ok(())
    }

    fn materialize(
        &mut self,
        schema: &Schema,
        kind: &str,
        ordinal: &mut u32,
        path: &Path,
    ) -> io::Result<()> {
        let shape = schema.shape(kind, path)?;
        for field in &shape.fields {
            if let Some(default) = &field.default
                && self.attribute(&field.name).is_none()
            {
                self.attributes.push((field.name.clone(), default.clone()));
            }
        }
        if schema.is_reference(kind) {
            index_reference(&mut self.text, ordinal);
        }
        for (name, value) in &mut self.attributes {
            if shape
                .fields
                .iter()
                .find(|field| field.name == *name)
                .is_some_and(|field| schema.is_reference(&field.kind))
            {
                index_reference(value, ordinal);
            }
        }
        for child in &mut self.children {
            let field = shape
                .children
                .iter()
                .find(|field| field.name == child.name)
                .ok_or_else(|| {
                    invalid(path, format!("unexpected {} in {}", child.name, self.name))
                })?;
            child.materialize(schema, &field.kind, ordinal, path)?;
        }
        Ok(())
    }

    fn write_xml(&self, xml: &mut String, root: bool) {
        xml.push('<');
        xml.push_str(&self.name);
        for (name, value) in &self.attributes {
            xml.push(' ');
            xml.push_str(name);
            xml.push_str("=\"");
            escape_xml(value, true, xml);
            xml.push('"');
        }
        if root {
            xml.push_str(" xmlns=\"uri:ea.com:eala:asset\"");
        }
        if self.children.is_empty() && self.text.is_empty() {
            xml.push_str(" />");
            return;
        }
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

fn update_flags(previous: &str, delta: &str, field: &str, path: &Path) -> io::Result<String> {
    let mut value = previous.to_owned();
    for change in delta.split_ascii_whitespace() {
        if let Some(flag) = change.strip_prefix('+').filter(|flag| !flag.is_empty()) {
            if !value.contains(flag) {
                value.push(' ');
                value.push_str(flag);
            }
        } else if let Some(flag) = change.strip_prefix('-').filter(|flag| !flag.is_empty()) {
            if !value.contains(flag) {
                return Err(invalid(
                    path,
                    format!("cannot remove missing {field} flag {flag}"),
                ));
            }
            value = value.replace(flag, "");
        } else {
            return Err(invalid(
                path,
                format!("invalid {field} flag override {change:?}"),
            ));
        }
    }
    Ok(value)
}

fn index_reference(value: &mut String, ordinal: &mut u32) {
    let name = value.trim().split('\\').next().unwrap_or_default();
    if !name.is_empty() {
        *value = format!("{name}\\{ordinal}");
        *ordinal += 1;
    }
}
mod weapon_set;
