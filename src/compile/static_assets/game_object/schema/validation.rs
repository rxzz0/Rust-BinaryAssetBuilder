use super::super::super::{finite_number, percentage, sage_angle, sage_time, sage_uint};
use super::{Schema, Shape};
use crate::compile::{SAGE_NS, invalid, warning};
use roxmltree::Node;
use std::collections::{HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::path::Path;

impl Schema {
    pub(in crate::compile) fn validate(
        &self,
        node: Node<'_, '_>,
        path: &Path,
    ) -> io::Result<Option<String>> {
        let Some((_, kind)) = self
            .assets
            .iter()
            .find(|(name, _)| *name == node.tag_name().name())
        else {
            return Ok(None);
        };
        let name = node.tag_name().name();
        let id = node.attribute("id").unwrap_or_default();
        let asset = format!("{name}:{id}");
        let mut validation = Validation {
            schema: self,
            path,
            asset: &asset,
            removals: Vec::new(),
            reported: HashSet::new(),
            missing: Vec::new(),
        };
        validation.check(node, kind, true)?;
        if !validation.missing.is_empty() {
            warning(format_args!(
                "Missing optional settings for '{asset}' in '{}': {}",
                display_path(path),
                validation.missing.join(", ")
            ));
        }
        if validation.removals.is_empty() {
            return Ok(None);
        }
        let mut xml = node.document().input_text().to_owned();
        validation
            .removals
            .sort_unstable_by_key(|range| range.start);
        for range in validation.removals.into_iter().rev() {
            xml.replace_range(range, "");
        }
        Ok(Some(xml))
    }

    fn valid_value(&self, kind: &str, value: &str, depth: u8) -> bool {
        if depth == 64 {
            return false;
        }
        let value = value.trim();
        match kind {
            "xs:boolean" => matches!(value, "true" | "false" | "0" | "1"),
            "SageBool" => {
                value.eq_ignore_ascii_case("true")
                    || value.eq_ignore_ascii_case("false")
                    || matches!(value, "0" | "1")
            }
            "xs:byte" => value.parse::<i8>().is_ok(),
            "xs:short" => value.parse::<i16>().is_ok(),
            "xs:int" => value.parse::<i32>().is_ok(),
            "xs:long" | "xs:integer" => value.parse::<i64>().is_ok(),
            "xs:unsignedByte" => value.parse::<u8>().is_ok(),
            "xs:unsignedShort" => value.parse::<u16>().is_ok(),
            "xs:unsignedInt" => value.parse::<u32>().is_ok(),
            "xs:unsignedLong" | "xs:nonNegativeInteger" => value.parse::<u64>().is_ok(),
            "xs:float" | "xs:double" => {
                value.parse::<f64>().is_ok() || matches!(value, "INF" | "-INF")
            }
            "xs:decimal" => {
                value.parse::<f64>().is_ok_and(f64::is_finite) && !value.contains(['e', 'E'])
            }
            "SageReal" => finite_number(value, kind, Path::new("")).is_ok(),
            "SageInt" => {
                value.parse::<i32>().is_ok()
                    || value
                        .strip_prefix("0x")
                        .is_some_and(|hex| u32::from_str_radix(hex, 16).is_ok())
            }
            "SageUnsignedInt" | "SageDurationUnsignedInt" | "DurationUnsignedInt" => {
                sage_uint(value, kind, Path::new("")).is_ok()
            }
            "Percentage" => percentage(value, kind, Path::new("")).is_ok(),
            "Angle" => sage_angle(value, kind, Path::new("")).is_ok(),
            "Time" => sage_time(value, kind, Path::new("")).is_ok(),
            "HashCode" => value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "InstanceId" => valid_instance_id(value),
            "AssetReference" => {
                let (name, ordinal) = value.split_once('\\').unwrap_or((value, ""));
                let (kind, name) = name.split_once(':').unwrap_or(("", name));
                (kind.is_empty() || valid_instance_id(kind))
                    && (name.is_empty() || valid_instance_id(name))
                    && ordinal.bytes().all(|byte| byte.is_ascii_digit())
            }
            "WeakReference" => value.is_empty() || valid_instance_id(value),
            "SageUnsignedShort" => sage_uint(value, kind, Path::new(""))
                .is_ok_and(|value| u16::try_from(value).is_ok()),
            _ => {
                let Some(entry) = self.types.get(kind) else {
                    return true;
                };
                let length = || {
                    let mut item_type = entry;
                    for _ in 0..64 {
                        if item_type.list_item.is_some() {
                            return value.split_ascii_whitespace().count();
                        }
                        let Some(base) = item_type
                            .base
                            .as_ref()
                            .and_then(|base| self.types.get(base))
                        else {
                            break;
                        };
                        item_type = base;
                    }
                    value.chars().count()
                };
                if entry.length.is_some_and(|expected| length() != expected)
                    || entry.min_length.is_some_and(|minimum| length() < minimum)
                    || entry.min_value.is_some_and(|minimum| {
                        !value.parse::<f64>().is_ok_and(|value| value >= minimum)
                    })
                    || entry.max_value.is_some_and(|maximum| {
                        !value.parse::<f64>().is_ok_and(|value| value <= maximum)
                    })
                {
                    return false;
                }
                if !entry.enumerators.is_empty() {
                    return entry.enumerators.iter().any(|item| item == value);
                }
                if let Some(item) = &entry.list_item {
                    return value
                        .split_ascii_whitespace()
                        .all(|value| self.valid_value(item, value, depth + 1));
                }
                entry
                    .base
                    .as_deref()
                    .is_none_or(|base| self.valid_value(base, value, depth + 1))
            }
        }
    }
}

struct Validation<'a> {
    schema: &'a Schema,
    path: &'a Path,
    asset: &'a str,
    removals: Vec<Range<usize>>,
    reported: HashSet<String>,
    missing: Vec<String>,
}

impl Validation<'_> {
    fn check(&mut self, node: Node<'_, '_>, kind: &str, required: bool) -> io::Result<()> {
        let element = node.tag_name().name();
        if !self
            .schema
            .valid_value(kind, node.text().unwrap_or_default(), 0)
        {
            let issue = format!("Invalid value for element '{element}'");
            return self.reject(required, &issue, node.range());
        }
        if !self.schema.types.contains_key(kind) {
            return Ok(());
        }
        let shape = self.schema.shape(kind, self.path)?;
        for field in &shape.fields {
            if let Some(attribute) = node
                .attributes()
                .find(|attr| attr.namespace().is_none() && attr.name() == field.name)
            {
                if !self.schema.valid_value(&field.kind, attribute.value(), 0) {
                    let issue = format!(
                        "Invalid value '{}' for attribute '{}' on '{}'",
                        attribute.value(),
                        field.name,
                        element
                    );
                    self.reject(field.minimum > 0, &issue, attribute.range())?;
                }
            } else if field.minimum > 0 {
                return Err(self.error(&format!(
                    "Missing required attribute '{}' on '{}'",
                    field.name, element
                )));
            } else if field.explicitly_optional
                && field.default.is_none()
                && field.name != "typeHashCode"
            {
                self.note_missing(format!("{element}.{}", field.name));
            }
        }
        self.check_children(node, &shape)
    }

    fn check_children(&mut self, node: Node<'_, '_>, shape: &Shape<'_>) -> io::Result<()> {
        let element = node.tag_name().name();
        let children = || {
            node.children()
                .filter(|child| child.tag_name().namespace() == Some(SAGE_NS))
        };
        let module_choice = shape.is_module_choice();
        let mut choices = 0;
        let mut counts = HashMap::<&str, usize>::new();
        for child in children() {
            let name = child.tag_name().name();
            let Some(field) = shape.children.iter().find(|field| field.name == name) else {
                if module_choice {
                    self.reject(
                        shape.choice_min > 0,
                        &format!("Unexpected module '{name}' in '{element}'"),
                        child.range(),
                    )?;
                }
                continue;
            };
            let count = counts.entry(name).or_default();
            *count += 1;
            choices += 1;
            if !shape.choice && *count > field.maximum {
                self.reject(
                    field.minimum > 0,
                    &format!("Too many '{}' elements in '{element}'", field.name),
                    child.range(),
                )?;
                continue;
            }
            if shape.choice && choices > shape.choice_max {
                self.reject(
                    shape.choice_min > 0,
                    &format!("Too many entries in '{element}'"),
                    child.range(),
                )?;
                continue;
            }
            let required = if shape.choice {
                shape.choice_min > 0
            } else {
                field.minimum > 0
            };
            self.check(child, &field.kind, required)?;
        }
        if shape.choice {
            if choices < shape.choice_min {
                return Err(self.error(&format!("Missing required entry in '{element}'")));
            }
            return Ok(());
        }
        for field in &shape.children {
            let count = counts.get(field.name.as_str()).copied().unwrap_or_default();
            if count < field.minimum {
                return Err(self.error(&format!(
                    "Missing required element '{}' in '{}'",
                    field.name, element
                )));
            }
            if count == 0
                && field.explicitly_optional
                && field.default.is_none()
                && self.schema.is_module_container(&field.kind)
            {
                self.note_missing(format!("{} module section", field.name));
            }
        }
        Ok(())
    }

    fn reject(&mut self, required: bool, issue: &str, range: Range<usize>) -> io::Result<()> {
        if required {
            return Err(self.error(issue));
        }
        self.warn(issue);
        self.removals.push(range);
        Ok(())
    }

    fn note_missing(&mut self, field: String) {
        if !self.missing.contains(&field) {
            self.missing.push(field);
        }
    }

    fn warn(&mut self, issue: &str) {
        if self.reported.insert(issue.to_owned()) {
            warning(format_args!(
                "{issue} for '{}' in '{}'",
                self.asset,
                display_path(self.path)
            ));
        }
    }

    fn error(&self, issue: &str) -> io::Error {
        invalid(self.path, format!("{issue} for '{}'", self.asset))
    }
}

fn valid_instance_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn display_path(path: &Path) -> std::borrow::Cow<'_, str> {
    if let Some(path) = path.to_str().and_then(|path| path.strip_prefix(r"\\?\")) {
        return path.into();
    }
    path.to_string_lossy()
}

impl Shape<'_> {
    pub(in crate::compile::static_assets::game_object) fn is_module_choice(&self) -> bool {
        self.choice
            && !self.children.is_empty()
            && self
                .children
                .iter()
                .any(|field| field.kind.ends_with("ModuleData"))
    }
}

impl Schema {
    fn is_module_container(&self, kind: &str) -> bool {
        self.types.get(kind).is_some_and(|entry| {
            entry.choice
                && !entry.children.is_empty()
                && entry
                    .children
                    .iter()
                    .any(|field| field.kind.ends_with("ModuleData"))
        })
    }
}
