# Rust BinaryAssetBuilder progress

- Checkpoint 250 encodes ActivateModuleSpecialPower and ActivateModuleSpecialAbilityUpdate, and adds the shared module-list fields to toggle states. The fixture matches EA byte for byte. All 241 tests and Clippy passed.
- Checkpoint 249 fixes mixed-case GameObject boolean values. Checkpoint 248 adds 13 weapon-effect variants through a shared layout.
- Surge Step 6 passes Shinobi's one-shot power activation and reaches SpecialAbilityUpdate. Next task is exposing the shared ability base directly.
