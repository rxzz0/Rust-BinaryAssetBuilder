# Rust BinaryAssetBuilder progress

- Checkpoint 225 encodes 28 GameObject scalar fields, including `ThreatLevel`. The EA fixture matches payload, relocations, imports, and instance hash. All 219 tests and Clippy passed.
- Checkpoint 224 adds XPointer child selection to XInclude. Checkpoint 223 encodes attribute modifiers. Checkpoint 222 matches EA upgrade inheritance. Checkpoint 221 encodes player templates.
- Surge Step 6 passes Allied Engineer’s scalar attributes and reaches `UnitTypeIcon`. Next task is encoding its asset reference and verifying its target encoder.
