# Rust BinaryAssetBuilder progress

- Checkpoint 286 adds all 29 JoypadCommandBarButtonTemplate variants. All 41 EA fixtures matched, including explicitly empty references. All 279 tests and Clippy passed.
- Checkpoints 284–285 add all four PlayerPowerButtonTemplate variants and compile inherited FX lists.
- Checkpoints 274–283 add locomotor and surface mappings, transformed display and upgrade fields, sweeping damage, animation sound support, collision, ore, JetAIUpdate, and cleared EVA events.

- Surge Step 6 passed inherited FX lists in 11.15 seconds of release compilation before it reached DamageNugget DamageArc on Steel Ronin. Next task is encoding that field.
