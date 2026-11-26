use super::binary::BinaryData;
use super::{
    Marshaled, encode_words, finite_number, sage_uint, significant_children, validate_attributes,
    write_import,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn destination_marker(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let default = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "DefaultRenderInfo")))
        .ok_or_else(|| invalid(path, "expected UIDecalDestinationMarker/DefaultRenderInfo"))?;
    let settings: Vec<_> = children.collect();
    if settings.is_empty() {
        return Err(invalid(
            path,
            "expected UIDecalDestinationMarker/DestinationTypeSettings",
        ));
    }

    let mut data = BinaryData {
        payload: vec![0; 32],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    marker_render_info(default, 4, &mut data, &mut result)?;
    let start = data.list(24, settings.len(), 24)?;
    for (index, setting) in settings.into_iter().enumerate() {
        if !setting.has_tag_name((SAGE_NS, "DestinationTypeSettings")) {
            return Err(invalid(path, "expected DestinationTypeSettings"));
        }
        validate_attributes(setting, &["DestinationType"], path)?;
        let destination = match required(setting, "DestinationType", path)?.trim() {
            "Normal" => 0,
            "Aggressive" => 1,
            value => return Err(invalid(path, format!("unknown DestinationType {value:?}"))),
        };
        let info = only_child(setting, "RenderInfo", path)?;
        let offset = start + index * 24;
        data.word(offset, destination);
        marker_render_info(info, offset + 4, &mut data, &mut result)?;
    }
    Ok(data.finish(result))
}

fn marker_render_info(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    validate_attributes(node, &["Model", "ZBias"], path)?;
    if let Some(model) = node
        .attribute("Model")
        .filter(|model| !model.trim().is_empty())
    {
        write_import(model, "BaseRenderAssetType", offset, data, result)?;
    }
    let bias = finite_number(node.attribute("ZBias").unwrap_or("0"), "ZBias", path)?;
    data.word(offset + 4, bias.to_bits());
    let color = only_child(node, "Color", path)?;
    validate_attributes(color, &["R", "G", "B"], path)?;
    if has_content(color) {
        return Err(invalid(
            path,
            "unexpected content in destination marker Color",
        ));
    }
    for (index, channel) in ["R", "G", "B"].into_iter().enumerate() {
        let value = finite_number(required(color, channel, path)?, channel, path)?;
        data.word(offset + 8 + index * 4, value.to_bits());
    }
    Ok(())
}

fn only_child<'a, 'input>(
    parent: Node<'a, 'input>,
    name: &str,
    path: &Path,
) -> io::Result<Node<'a, 'input>> {
    let mut children = significant_children(parent);
    children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, name)) && children.next().is_none())
        .ok_or_else(|| {
            invalid(
                path,
                format!("expected one {}/{name}", parent.tag_name().name()),
            )
        })
}

pub(super) fn dynamics_settings(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &[
            "id",
            "MaximumObjects",
            "MaximumContacts",
            "MaximumJoints",
            "CreateGlobalIsland",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected content in DynamicsSettings"));
    }
    let maximum = |name, default| sage_uint(node.attribute(name).unwrap_or(default), name, path);
    let island = super::fx_list::boolean(node, "CreateGlobalIsland", false, path)?;
    Ok(Marshaled {
        // The shipped Uprising stock asset and all campaign overrides use 1024
        // for the allocation slot between MaximumContacts and MaximumJoints.
        payload: encode_words([
            0,
            maximum("MaximumObjects", "1024")?,
            maximum("MaximumContacts", "2048")?,
            1024,
            maximum("MaximumJoints", "256")?,
            u32::from(island),
        ]),
        ..Marshaled::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use roxmltree::Document;

    #[test]
    fn destination_marker_matches_ea_model_imports() {
        let xml = r#"<UIDecalDestinationMarker xmlns="uri:ea.com:eala:asset" id="Marker">
            <DefaultRenderInfo Model="SCMoveHint_Pink"><Color R="1" G="1" B="1"/></DefaultRenderInfo>
            <DestinationTypeSettings DestinationType="Aggressive">
                <RenderInfo Model="SCMoveHint_Attack_Pink"><Color R="1" G="1" B="1"/></RenderInfo>
            </DestinationTypeSettings>
        </UIDecalDestinationMarker>"#;
        let document = Document::parse(xml).unwrap();
        let data = destination_marker(document.root_element(), Path::new("overrides.xml")).unwrap();
        let white = 1.0f32.to_bits();
        assert_eq!(
            data.payload,
            encode_words([
                0, 1, 0, white, white, white, 1, 32, 1, 2, 0, white, white, white
            ])
        );
        assert_eq!(data.relocations, [28, u32::MAX]);
        assert_eq!(data.imports, [4, 36, u32::MAX]);
        let instances: Vec<_> = data
            .references
            .iter()
            .map(|reference| reference.id.instance_id)
            .collect();
        assert_eq!(instances, [0xe61b_e3b5, 0xe301_5dfb]);
    }

    #[test]
    fn dynamics_settings_matches_shipped_campaign_allocations() {
        for (attributes, expected) in [
            (
                r#"MaximumContacts="2560" MaximumJoints="256" CreateGlobalIsland="true""#,
                [0, 1024, 2560, 1024, 256, 1],
            ),
            (
                r#"MaximumObjects="1536" MaximumContacts="2560" CreateGlobalIsland="true""#,
                [0, 1536, 2560, 1024, 256, 1],
            ),
            (
                r#"MaximumObjects="2048" MaximumContacts="4096" MaximumJoints="1024" CreateGlobalIsland="true""#,
                [0, 2048, 4096, 1024, 1024, 1],
            ),
            ("", [0, 1024, 2048, 1024, 256, 0]),
        ] {
            let xml = format!(r#"<DynamicsSettings id="Settings_Dynamics" {attributes}/>"#);
            let document = Document::parse(&xml).unwrap();
            let data =
                dynamics_settings(document.root_element(), Path::new("overrides.xml")).unwrap();
            assert_eq!(data.payload, encode_words(expected));
            assert!(data.imports.is_empty());
            assert!(data.relocations.is_empty());
        }
    }
}
