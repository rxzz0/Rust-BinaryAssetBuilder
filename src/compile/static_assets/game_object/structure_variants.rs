use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, sage_int, sage_uint, significant_children,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::passive_area::reference;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const COMPOSITE: u32 = 0xa112_a404;
const WALL: u32 = 0xf3f8_adfa;
const MINEFIELD: u32 = 0xb53a_18ad;
const REAL: u32 = 0x6458_64f3;
const INT: u32 = 0x23e6_50b8;
const UINT: u32 = 0x7631_77a4;
const RELATIONSHIP: u32 = 0x5c89_81a9;
const WEAK_OBJECT: u32 = 0x148e_d75d;
const GAME_OBJECT: u32 = 0xbfab_9075;
const WEAPON: u32 = 0xd046_dd22;
const FX: u32 = 0x0d1c_8e83;
const COMPOSITE_FIELDS: &[(&str, u32, usize, &str)] = &[
    ("ThingTemplate", GAME_OBJECT, 8, ""),
    ("Count", UINT, 12, ""),
    ("BuildableDistance", REAL, 16, ""),
];
const WALL_FIELDS: &[(&str, u32, usize, &str)] = &[
    ("HubCapTemplateId", WEAK_OBJECT, 8, ""),
    ("DefaultSegmentTemplateId", WEAK_OBJECT, 12, ""),
    ("CliffCapTemplateId", WEAK_OBJECT, 16, ""),
    ("ShoreCapTemplateId", WEAK_OBJECT, 20, ""),
    ("BorderCapTemplateId", WEAK_OBJECT, 24, ""),
    ("ElevatedSegmentTemplateId", WEAK_OBJECT, 28, ""),
    ("BuilderRadius", REAL, 32, "0"),
    ("MaxBuildoutDistance", REAL, 36, "0"),
    ("Options", UINT, 40, "0"),
    ("StaggeredBuildFactor", INT, 44, "0"),
];
const MINEFIELD_FIELDS: &[(&str, u32, usize, &str)] = &[
    ("DetonationWeapon", WEAPON, 8, ""),
    ("DetonatedBy", RELATIONSHIP, 12, ""),
    ("DetonationFX", FX, 16, ""),
    ("NumVirtualMines", UINT, 20, "0"),
    ("RepeatDetonateMoveThresh", REAL, 24, "5"),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let name = node.tag_name().name();
    let (kind, size, fields) = match name {
        "CompositeStructureInfoModule" => (COMPOSITE, 0x18, COMPOSITE_FIELDS),
        "WallHubBehavior" => (WALL, 0x38, WALL_FIELDS),
        "MinefieldBehavior" => (MINEFIELD, 0x1c, MINEFIELD_FIELDS),
        _ => return Err(invalid(path, format!("unexpected behavior {name}"))),
    };
    let allowed: Vec<_> = std::iter::once("id")
        .chain(fields.iter().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if kind == COMPOSITE {
        for &(name, ..) in fields {
            required(node, name, path)?;
        }
    }
    let start = data.allocate(pointer, size)?;
    let mut entries = vec![module_base(node, kind, start, data, kind != COMPOSITE)?];

    for attr in node.attributes() {
        let Some(&(_, tag, offset, _)) = fields.iter().find(|&&(name, ..)| name == attr.name())
        else {
            continue;
        };
        let value = attr.value();
        let reference_kind = match tag {
            GAME_OBJECT => "GameObject",
            WEAPON => "WeaponTemplate",
            FX => "FXList",
            WEAK_OBJECT if !value.trim().is_empty() => {
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, "GameObject", path)?;
                reference_types.push(id.type_id);
                data.word(start + offset, id.instance_id);
                continue;
            }
            _ => continue,
        };
        let ordinal = reference(value, reference_kind, path, references, reference_types)?;
        data.word(start + offset, ordinal);
    }
    for &(field, tag, offset, default) in fields {
        let offset = start + offset;
        let value = node.attribute(field).unwrap_or(default);
        let bits = match tag {
            REAL => Some(finite_number(value, field, path)?.to_bits()),
            INT => Some(sage_int(value, field, path)?),
            UINT => Some(sage_uint(value, field, path)?),
            RELATIONSHIP => Some(super::parking::relationship_mask(value, path)?),
            _ => None,
        };
        if let Some(bits) = bits {
            data.word(offset, bits);
        }
        entries.push(Entry::named(field, tag, Target::Data(offset)));
    }
    match kind {
        COMPOSITE => {
            let mut children = significant_children(node);
            let shadow = match children.next() {
                Some(child)
                    if child.has_tag_name((SAGE_NS, "ConnectionShadowInfo"))
                        && children.next().is_none() =>
                {
                    Target::Table(super::shadow::encode(
                        child,
                        start + 0x14,
                        data,
                        references,
                        reference_types,
                    )?)
                }
                Some(_) => {
                    return Err(invalid(
                        path,
                        "unexpected CompositeStructureInfoModule child",
                    ));
                }
                None => Target::Null,
            };
            entries.push(Entry::named("ConnectionShadowInfo", 0x82fe_7e88, shadow));
        }
        WALL => {
            let ids = significant_children(node)
                .map(|child| {
                    if !child.has_tag_name((SAGE_NS, "SegmentPattern")) {
                        return Err(invalid(path, "expected WallHubBehavior SegmentPattern"));
                    }
                    super::spawn::weak_reference(child, "GameObject", data, reference_types)
                })
                .collect::<io::Result<Vec<_>>>()?;
            let segments = super::spawn::id_list(data, start + 0x30, ids)?;
            entries.push(Entry::named(
                "SegmentPattern",
                WEAK_OBJECT,
                Target::Table(segments),
            ));
        }
        _ if has_content(node) => {
            return Err(invalid(path, "unexpected MinefieldBehavior content"));
        }
        _ => {}
    }
    Ok((kind, Table::record(entries)))
}
