use super::super::game_object::{weak_xml_reference, write_model_conditions};
use super::{BinaryData, Entry, Marshaled, SAGE_NS, Target, finite_number, invalid, percentage};
use roxmltree::Node;
use std::io;
use std::iter::Peekable;

#[derive(Clone, Copy)]
pub(super) enum Variant {
    Spawn,
    LeechDamage,
    LeechMaxHealth,
    GrapplingHook,
}

impl Variant {
    pub(super) fn for_node(node: Node<'_, '_>) -> Option<Self> {
        match (node.tag_name().namespace(), node.tag_name().name()) {
            (Some(SAGE_NS), "DamageAndSpawnNugget") => Some(Self::Spawn),
            (Some(SAGE_NS), "LeechHealthDamageNugget") => Some(Self::LeechDamage),
            (Some(SAGE_NS), "LeechPercentMaxHealthDamageNugget") => Some(Self::LeechMaxHealth),
            (Some(SAGE_NS), "GrapplingHookNugget") => Some(Self::GrapplingHook),
            _ => None,
        }
    }

    pub(super) fn type_id(self) -> u32 {
        match self {
            Self::Spawn => 0xc2ab_8308,
            Self::LeechDamage => 0xef26_7a3e,
            Self::LeechMaxHealth => 0x36ef_b80f,
            Self::GrapplingHook => 0x4127_f7e0,
        }
    }

    pub(super) fn size(self) -> usize {
        match self {
            Self::Spawn => 0xdc,
            Self::LeechDamage | Self::LeechMaxHealth => 0x9c,
            Self::GrapplingHook => 0x98,
        }
    }

    pub(super) fn attributes(self) -> &'static [&'static str] {
        match self {
            Self::Spawn => &["SpawnProbability", "SpawnedModelConditionFlags"],
            Self::LeechDamage => &["PercentDamageToHeal"],
            Self::LeechMaxHealth => &["PercentMaxHealthToTake"],
            Self::GrapplingHook => &[],
        }
    }

    pub(super) fn encode_fields<'a, 'input: 'a>(
        self,
        node: Node<'a, 'input>,
        start: usize,
        data: &mut BinaryData<'_>,
        out: &mut Marshaled,
        children: &mut Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    ) -> io::Result<Vec<Entry>> {
        let path = data.path;
        let Some(&field) = self.attributes().first() else {
            return Ok(Vec::new());
        };
        let value = if matches!(self, Self::Spawn) {
            finite_number(node.attribute(field).unwrap_or("1"), field, path)?
        } else {
            percentage(node.attribute(field).unwrap_or("100%"), field, path)?
        };
        data.word(start + 0x98, value.to_bits());
        let mut entries = vec![Entry::named(field, 0x6458_64f3, Target::Data(start + 0x98))];
        if let Self::Spawn = self {
            write_model_conditions(
                node.attribute("SpawnedModelConditionFlags").unwrap_or(""),
                start + 0x9c,
                data,
            )?;
            let template = children
                .next_if(|child| child.has_tag_name((SAGE_NS, "SpawnTemplate")))
                .ok_or_else(|| invalid(path, "DamageAndSpawnNugget requires SpawnTemplate"))?;
            let template =
                weak_xml_reference(template, "GameObject", data, &mut out.reference_types)?;
            data.word(start + 0xd8, template);
            entries.extend([
                Entry::named(
                    "SpawnedModelConditionFlags",
                    0x67db_bfc9,
                    Target::Data(start + 0x9c),
                ),
                Entry::named("SpawnTemplate", 0x148e_d75d, Target::Data(start + 0xd8)),
            ]);
        }
        Ok(entries)
    }
}
