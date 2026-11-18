# Rust BinaryAssetBuilder progress

- Checkpoint 251 encodes direct SpecialAbilityUpdate and TimedOpacityUpdate with default and configured fields. The fixture matches EA byte for byte. All 242 tests and Clippy passed.
- Checkpoint 250 encodes the module activation variants and toggle-state triggers. Checkpoint 249 fixes mixed-case GameObject boolean values.
- Surge Step 6 passes Shinobi's ability and opacity updates and reaches TrackIncomingWeaponFireBehavior. Next task is encoding the two damage tracking modules from the SDK schema.
