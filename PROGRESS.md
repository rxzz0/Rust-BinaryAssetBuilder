# Rust BinaryAssetBuilder progress

- Checkpoint 238 encodes SkirmishAiInfo attributes and StateWeapon settings for toggle abilities. It reuses the special-power weapon encoder. Both toggle fixtures match EA byte for byte. All 230 tests and Clippy passed.
- Checkpoint 237 encodes scriptable draw track marks, particles, and flags. Checkpoint 236 encodes two weapon nuggets and shared effect conditions.
- Surge Step 6 passes the Hammer Tank's toggle ability and reaches ConvergingLaserState. Next task is verifying and encoding that behavior.
