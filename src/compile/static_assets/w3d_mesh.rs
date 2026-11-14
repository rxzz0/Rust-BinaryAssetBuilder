use super::binary::BinaryData;
use super::global_ui::enum_value;
use super::{
    Marshaled, finite_number, required_element, significant_children, text_value,
    validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io;
use std::path::Path;

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(
        node,
        &["id", "Hidden", "CastShadow", "GeometryType", "SortLevel"],
        path,
    )?;
    let mut children = significant_children(node).peekable();
    let bounds = required_element(&mut children, "BoundingBox", path)?;
    let sphere = required_element(&mut children, "BoundingSphere", path)?;
    let mut streams = Vec::new();
    while let Some(child) = children.next_if(|child| !child.has_tag_name((SAGE_NS, "Triangles"))) {
        streams.push(child);
    }
    let triangles = required_element(&mut children, "Triangles", path)?;
    let shader = required_element(&mut children, "FXShader", path)?;
    let tree = children.next_if(|child| child.has_tag_name((SAGE_NS, "AABTree")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected W3DMesh child"));
    }
    let mut binary = BinaryData {
        payload: vec![0; 0x60],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    binary.word(
        8,
        enum_value(
            node.attribute("GeometryType").unwrap_or("Normal"),
            &["Normal", "Skin", "CameraAligned", "CameraOriented"],
            path,
        )?,
    );
    for (name, offset) in [("Hidden", 0x5c), ("CastShadow", 0x5d)] {
        binary.payload[offset] = match node.attribute(name).unwrap_or("false").trim() {
            "true" | "1" => 1,
            "false" | "0" => 0,
            _ => return Err(invalid(path, format!("invalid W3DMesh {name}"))),
        };
    }
    binary.payload[0x5e] = node
        .attribute("SortLevel")
        .unwrap_or("0")
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|&value| value <= 32)
        .ok_or_else(|| invalid(path, "invalid W3DMesh SortLevel"))?;
    write_box(bounds, 0xc, &mut binary)?;
    validate_attributes(sphere, &["Radius"], path)?;
    binary.word(
        0x24,
        finite_number(sphere.attribute("Radius").unwrap_or("0"), "Radius", path)?.to_bits(),
    );
    let mut center = significant_children(sphere);
    write_vector(
        required_element(&mut center, "Center", path)?,
        0x28,
        &mut binary,
    )?;
    end(center, path)?;

    write_triangles(triangles, &mut binary)?;
    super::game_object::material::encode_render_material(shader, 0x3c, &mut binary, &mut result)?;
    if let Some(tree) = tree {
        write_tree(tree, &mut binary)?;
    }
    write_vertices(&streams, &mut binary)?;
    // The render plugin traverses root fields in marshal order, with the custom
    // vertex-data pointer after the ordinary W3DMesh fields.
    binary.relocations.sort_by_key(|&offset| match offset {
        4 => 0x60,
        0..0x60 => offset,
        _ => 0x61,
    });
    binary.relocations.push(u32::MAX);
    if !result.imports.is_empty() {
        result.imports.push(u32::MAX);
    }
    result.payload = binary.payload;
    result.relocations = binary.relocations;
    Ok(result)
}

fn write_vector(node: Node<'_, '_>, offset: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    for (index, value) in components::<3>(node, ["X", "Y", "Z"], data.path)?
        .into_iter()
        .enumerate()
    {
        data.word(offset + 4 * index, value.to_bits());
    }
    Ok(())
}

fn components<const N: usize>(
    node: Node<'_, '_>,
    names: [&str; N],
    path: &Path,
) -> io::Result<[f32; N]> {
    validate_attributes(node, &names, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected vertex component content"));
    }
    let mut result = [0.0; N];
    for (target, name) in result.iter_mut().zip(names) {
        *target = finite_number(required(node, name, path)?, name, path)?;
    }
    Ok(result)
}

fn write_box(node: Node<'_, '_>, offset: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let mut children = significant_children(node);
    write_vector(
        required_element(&mut children, "Min", data.path)?,
        offset,
        data,
    )?;
    write_vector(
        required_element(&mut children, "Max", data.path)?,
        offset + 12,
        data,
    )?;
    end(children, data.path)
}

fn write_triangles(node: Node<'_, '_>, data: &mut BinaryData<'_>) -> io::Result<()> {
    let path = data.path;
    let triangles = items(node, "T", path)?;
    let start = data.list(0x34, triangles.len(), 24)?;
    for (index, triangle) in triangles.into_iter().enumerate() {
        let offset = start + 24 * index;
        let mut children = significant_children(triangle);
        let vertices = data.list(offset, 3, 4)?;
        for index in 0..3 {
            let vertex = required_element(&mut children, "V", path)?;
            data.word(
                vertices + 4 * index,
                uint(text_value(vertex, path)?, "triangle vertex", path)?,
            );
        }
        write_vector(
            required_element(&mut children, "Nrm", path)?,
            offset + 8,
            data,
        )?;
        let distance = required_element(&mut children, "Dist", path)?;
        data.word(
            offset + 20,
            finite_number(text_value(distance, path)?, "triangle distance", path)?.to_bits(),
        );
        end(children, path)?;
    }
    Ok(())
}

fn write_tree(node: Node<'_, '_>, data: &mut BinaryData<'_>) -> io::Result<()> {
    let path = data.path;
    let start = data.allocate(0x58, 16)?;
    let mut children = significant_children(node);
    let polys = items(
        required_element(&mut children, "PolyIndices", path)?,
        "P",
        path,
    )?;
    let first = data.list(start, polys.len(), 4)?;
    for (index, poly) in polys.into_iter().enumerate() {
        data.word(
            first + 4 * index,
            uint(text_value(poly, path)?, "polygon index", path)?,
        );
    }
    let nodes: Vec<_> = children.collect();
    let first = data.list(start + 8, nodes.len(), 32)?;
    for (index, node) in nodes.into_iter().enumerate() {
        if !node.has_tag_name((SAGE_NS, "Node")) {
            return Err(invalid(path, "unexpected AABTree child"));
        }
        let offset = first + index * 32;
        let mut children = significant_children(node);
        write_vector(required_element(&mut children, "Min", path)?, offset, data)?;
        write_vector(
            required_element(&mut children, "Max", path)?,
            offset + 12,
            data,
        )?;
        let branch = children
            .next()
            .ok_or_else(|| invalid(path, "AABTree node requires Children or Polys"))?;
        let (pointer, attrs) = if branch.has_tag_name((SAGE_NS, "Children")) {
            (offset + 24, ["Front", "Back"])
        } else if branch.has_tag_name((SAGE_NS, "Polys")) {
            (offset + 28, ["Begin", "Count"])
        } else {
            return Err(invalid(path, "unexpected AABTree node child"));
        };
        validate_attributes(branch, &attrs, path)?;
        let target = data.allocate(pointer, 8)?;
        for (index, field) in attrs.into_iter().enumerate() {
            data.word(
                target + 4 * index,
                uint(required(branch, field, path)?, field, path)?,
            );
        }
        end(children, path)?;
    }
    Ok(())
}

struct VertexStream {
    usage: char,
    index: usize,
    format: &'static str,
    packed: Vec<u8>,
    width: usize,
}

fn write_vertices(nodes: &[Node<'_, '_>], data: &mut BinaryData<'_>) -> io::Result<()> {
    let path = data.path;
    let mut ordered = Vec::new();
    let mut remaining = nodes.iter().copied().peekable();
    let positions = read_groups(&mut remaining, "Vertices");
    let count = positions
        .first()
        .ok_or_else(|| invalid(path, "W3DMesh requires Vertices"))?
        .children()
        .filter(Node::is_element)
        .count();
    if count == 0 || positions.len() > 2 {
        return Err(invalid(
            path,
            "invalid W3DMesh vertex count or stream count",
        ));
    }
    for (index, position) in positions.iter().enumerate() {
        ordered.push(vector_stream(
            *position,
            "V",
            'p',
            index,
            ["X", "Y", "Z"],
            count,
            path,
        )?);
    }
    for (index, normal) in read_groups(&mut remaining, "Normals")
        .into_iter()
        .enumerate()
    {
        ordered.push(vector_stream(
            normal,
            "N",
            'n',
            index,
            ["X", "Y", "Z"],
            count,
            path,
        )?);
    }
    let tangent = remaining.next_if(|node| node.has_tag_name((SAGE_NS, "Tangents")));
    let binormal = remaining.next_if(|node| node.has_tag_name((SAGE_NS, "Binormals")));
    let colors = remaining.next_if(|node| node.has_tag_name((SAGE_NS, "VertexColors")));
    let packed = match colors {
        Some(colors) => {
            let colors = checked_items(colors, "C", count, path)?;
            let mut packed = Vec::with_capacity(count * 4);
            for color in colors {
                let rgba = components(color, ["R", "G", "B", "A"], path)?;
                packed.extend(
                    [rgba[2], rgba[1], rgba[0], rgba[3]]
                        .map(|value| (f64::from(value) * 255.0) as u8),
                );
            }
            packed
        }
        None => vec![255; count * 4],
    };
    ordered.push(VertexStream {
        usage: 'c',
        index: 0,
        format: "4u8n",
        packed,
        width: 4,
    });
    for (stream, item, usage) in [(tangent, "T", 'a'), (binormal, "B", 'b')] {
        if let Some(stream) = stream.filter(|_| tangent.is_some() && binormal.is_some()) {
            ordered.push(vector_stream(
                stream,
                item,
                usage,
                0,
                ["X", "Y", "Z"],
                count,
                path,
            )?);
        }
    }
    for (index, tex) in read_groups(&mut remaining, "TexCoords")
        .into_iter()
        .enumerate()
    {
        ordered.push(vector_stream(
            tex,
            "T",
            't',
            index,
            ["X", "Y"],
            count,
            path,
        )?);
    }
    let influences = read_groups(&mut remaining, "BoneInfluences");
    let bones = append_skin(&mut ordered, &influences, positions.len(), count, path)?;
    remaining.next_if(|node| node.has_tag_name((SAGE_NS, "ShadeIndices")));
    if remaining.next().is_some() {
        return Err(invalid(path, "unexpected W3DMesh vertex stream"));
    }

    let start = data.allocate(4, 28)?;
    let stride: usize = ordered.iter().map(|stream| stream.width).sum();
    let size = stride
        .checked_mul(count)
        .ok_or_else(|| invalid(path, "W3DMesh vertex stream too large"))?;
    let vertices = data.allocate(start + 8, size)?;
    data.word(start, count as u32);
    data.word(start + 4, stride as u32);
    let mut offset = 0;
    let mut declaration = String::new();
    for stream in ordered {
        if !declaration.is_empty() {
            declaration.push(' ');
        }
        write!(
            declaration,
            "{}{}:{offset:02X}:{}",
            stream.usage, stream.index, stream.format
        )
        .unwrap();
        for (index, bytes) in stream.packed.chunks_exact(stream.width).enumerate() {
            let dest = vertices + index * stride + offset;
            data.payload[dest..dest + stream.width].copy_from_slice(bytes);
        }
        offset += stream.width;
    }
    let declaration_size = (declaration.len() + 1).next_multiple_of(4);
    let target = data.allocate(start + 16, declaration_size)?;
    data.word(start + 12, declaration.len() as u32);
    data.payload[target..target + declaration.len()].copy_from_slice(declaration.as_bytes());
    let target = data.allocate(start + 24, (bones.len() * 2).next_multiple_of(4))?;
    data.word(start + 20, bones.len() as u32);
    for (index, bone) in bones.into_iter().enumerate() {
        let offset = target + 2 * index;
        data.payload[offset..offset + 2].copy_from_slice(&bone.to_le_bytes());
    }
    Ok(())
}

fn read_groups<'a, 'input: 'a>(
    nodes: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    name: &str,
) -> Vec<Node<'a, 'input>> {
    let mut groups = Vec::new();
    while let Some(node) = nodes.next_if(|node| node.has_tag_name((SAGE_NS, name))) {
        groups.push(node);
    }
    groups
}

fn vector_stream<const N: usize>(
    node: Node<'_, '_>,
    item: &str,
    usage: char,
    index: usize,
    axes: [&str; N],
    count: usize,
    path: &Path,
) -> io::Result<VertexStream> {
    let mut packed = Vec::with_capacity(count * N * 4);
    for node in checked_items(node, item, count, path)? {
        let mut values = components(node, axes, path)?;
        if usage == 't' {
            values[1] = 1.0 - values[1];
        }
        packed.extend(values.into_iter().flat_map(f32::to_le_bytes));
    }
    Ok(VertexStream {
        usage,
        index,
        format: if N == 2 { "2f32" } else { "3f32" },
        packed,
        width: N * 4,
    })
}

fn append_skin(
    streams: &mut Vec<VertexStream>,
    influences: &[Node<'_, '_>],
    position_count: usize,
    count: usize,
    path: &Path,
) -> io::Result<Vec<u16>> {
    if influences.is_empty() {
        return Ok(Vec::new());
    }
    let mut bones = BTreeSet::new();
    let mut groups = Vec::new();
    for node in influences {
        let mut group = Vec::with_capacity(count);
        for influence in checked_items(*node, "I", count, path)? {
            validate_attributes(influence, &["Bone", "Weight"], path)?;
            let bone = influence
                .attribute("Bone")
                .unwrap_or("0")
                .trim()
                .parse::<u16>()
                .map_err(|_| invalid(path, "invalid W3DMesh bone index"))?;
            let weight =
                finite_number(influence.attribute("Weight").unwrap_or("0"), "Weight", path)?;
            bones.insert(bone);
            group.push((bone, weight));
        }
        groups.push(group);
    }
    let bones: Vec<_> = bones.into_iter().collect();
    if bones.len() > 256 || groups.len() > 2 {
        return Err(invalid(path, "too many W3DMesh bones or influence streams"));
    }
    let used = groups.len().min(position_count);
    let mut packed = Vec::with_capacity(count * 4);
    for vertex in 0..count {
        let mut indexes = [0; 4];
        for index in 0..used {
            indexes[index] = bones.binary_search(&groups[index][vertex].0).unwrap() as u8;
        }
        packed.extend(indexes);
    }
    streams.push(VertexStream {
        usage: 'i',
        index: 0,
        format: "4u8",
        packed,
        width: 4,
    });
    if used > 1 {
        let mut weights = Vec::with_capacity(count * 8);
        for vertex in 0..count {
            for group in &groups[..used] {
                weights.extend(group[vertex].1.to_le_bytes());
            }
        }
        streams.push(VertexStream {
            usage: 'w',
            index: 0,
            format: "2f32",
            packed: weights,
            width: 8,
        });
    }
    Ok(bones)
}

fn checked_items<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    item: &str,
    count: usize,
    path: &Path,
) -> io::Result<Vec<Node<'a, 'input>>> {
    let values = items(node, item, path)?;
    if values.len() != count {
        return Err(invalid(
            path,
            "W3DMesh vertex stream has mismatched item count",
        ));
    }
    Ok(values)
}

fn items<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    item: &str,
    path: &Path,
) -> io::Result<Vec<Node<'a, 'input>>> {
    validate_attributes(node, &[], path)?;
    let values: Vec<_> = significant_children(node).collect();
    if values
        .iter()
        .any(|node| !node.has_tag_name((SAGE_NS, item)))
    {
        return Err(invalid(path, format!("expected W3DMesh {item}")));
    }
    Ok(values)
}

fn end<'a, 'input: 'a>(
    mut children: impl Iterator<Item = Node<'a, 'input>>,
    path: &Path,
) -> io::Result<()> {
    if children.next().is_some() {
        return Err(invalid(path, "unexpected W3DMesh child"));
    }
    Ok(())
}

fn uint(value: &str, field: &str, path: &Path) -> io::Result<u32> {
    value
        .trim()
        .parse()
        .map_err(|_| invalid(path, format!("invalid {field}")))
}
