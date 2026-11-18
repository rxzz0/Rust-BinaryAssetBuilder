# Rust BinaryAssetBuilder progress

- Checkpoint 241 encodes FireWeaponWhenDead probabilities, weapon offsets, and inherited upgrade fields. The fixture matches EA byte for byte. All 232 tests and Clippy passed.
- Checkpoint 240 encodes ObjectCreationUpgrade and SlaveWatcherBehavior. Checkpoint 239 adds the two LaserState variants and verifies the existing StreamState encoder.
- Surge Step 6 passes the death-weapon probabilities and reaches AudioLoopUpgrade. Next task is encoding its sound reference and lifetime settings.
