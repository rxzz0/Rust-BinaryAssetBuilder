# Rust BinaryAssetBuilder progress

- Checkpoint 259 encodes FXParticleSystemTemplate, including keyframes, wind, references, and all 27 schema-reachable physics, draw, volume, velocity, event, and update variants. The 30 oracle assets across three fixtures match EA byte for byte. All 250 tests and Clippy passed.
- Checkpoint 258 adds Multisound. Checkpoint 257 adds object-specific voice entries.

- Surge Step 6 passes the Yuriko particle templates and reaches SovietTeslaTank's TankDraw. Next task is the native TankDraw encoder using the shared model-draw layout.
