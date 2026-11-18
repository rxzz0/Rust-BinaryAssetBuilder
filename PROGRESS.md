# Rust BinaryAssetBuilder progress

- Checkpoint 240 adds ObjectCreationUpgrade and SlaveWatcherBehavior. The fixture covers both modules, inherited upgrade lists, and strong and weak references, and matches EA byte for byte. All 231 tests and Clippy passed.
- Checkpoint 239 encodes laser-state variants and StreamState. Checkpoint 238 encodes toggle ability AI settings.
- Surge Step 6 passes the Hammer Tank's spawn upgrade and reaches FireWeaponWhenDead WeaponFireProbability. Next task is encoding the probability entries and checking the death-module schema.
