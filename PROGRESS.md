# Rust BinaryAssetBuilder progress

- Checkpoint 260 encodes TankDraw through the shared scripted model draw path, including terrain particle references and articulation bones. The EA fixture matches byte for byte. All 251 tests and Clippy passed.
- Checkpoint 259 adds FXParticleSystemTemplate and all 27 particle module variants.

- Surge Step 6 passes SovietTeslaTank's TankDraw and reaches an AudioEvent with VolumeSliderMultiplier. Next task is completing the common single-sound child layouts.
