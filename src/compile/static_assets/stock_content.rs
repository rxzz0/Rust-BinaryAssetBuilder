use super::binary::BinaryData;
use super::{Marshaled, significant_children, text_value, validate_attributes, write_import};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn engaging_content(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let names = ["TitleId", "DescId", "ImageId"];
    validate_attributes(node, &["id", "TitleId", "DescId", "ImageId"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EngagingContent content"));
    }
    let mut data = BinaryData {
        payload: vec![0; 28],
        relocations: Vec::new(),
        path,
    };
    for (index, name) in names.into_iter().enumerate() {
        data.string(4 + index * 8, required(node, name, path)?)?;
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn engaging_group(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let items: Vec<_> = significant_children(node).collect();
    if items.is_empty() {
        return Err(invalid(
            path,
            "EngagingContentGroup requires at least one EngagingContent",
        ));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let start = data.list(4, items.len(), 4)?;
    for (index, item) in items.into_iter().enumerate() {
        if !item.has_tag_name((SAGE_NS, "EngagingContent")) || item.attributes().len() != 0 {
            return Err(invalid(path, "unexpected EngagingContentGroup child"));
        }
        write_import(
            text_value(item, path)?,
            "EngagingContent",
            start + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    Ok(data.finish(result))
}

pub(super) fn engaging_sequence(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &["id", "MapName", "RequiredContent", "RandomContent"],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EngagingContentSequence content"));
    }
    let mut data = BinaryData {
        payload: vec![0; 20],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.string(4, required(node, "MapName", path)?)?;
    for (name, offset) in [("RequiredContent", 12), ("RandomContent", 16)] {
        if let Some(value) = node.attribute(name) {
            write_import(
                value,
                "EngagingContentGroup",
                offset,
                &mut data,
                &mut result,
            )?;
        }
    }
    Ok(data.finish(result))
}

pub(super) fn tutorial_map_data(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let lessons: Vec<_> = significant_children(node).collect();
    if lessons.is_empty() {
        return Err(invalid(
            path,
            "TutorialMapData requires at least one Lesson",
        ));
    }
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let start = data.list(4, lessons.len(), 24)?;
    let fields = ["Map", "Summary", "ImageName"];
    for (index, lesson) in lessons.into_iter().enumerate() {
        if !lesson.has_tag_name((SAGE_NS, "Lesson")) || has_content(lesson) {
            return Err(invalid(path, "unexpected TutorialMapData child"));
        }
        validate_attributes(lesson, &fields, path)?;
        for (field, name) in fields.into_iter().enumerate() {
            if let Some(value) = lesson.attribute(name) {
                data.string(start + index * 24 + field * 8, value)?;
            }
        }
    }
    Ok(data.finish(Marshaled::default()))
}
