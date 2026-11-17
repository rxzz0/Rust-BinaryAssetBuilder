# Rust BinaryAssetBuilder progress

- Checkpoint 226 encodes `UnitTypeIcon`, `PackedTextureImage`, and the GameObject icon reference. Four assets match EA byte for byte. All 220 tests and Clippy passed.
- Checkpoint 225 encodes 28 GameObject scalar fields. Checkpoint 224 adds XPointer child selection. Checkpoint 223 encodes attribute modifiers. Checkpoint 222 matches EA upgrade inheritance.
- Surge Step 6 reaches the Allied Engineer’s `CrushDie` behavior. Next task is encoding its behavior layout and comparing it against EA.
