# Rust BinaryAssetBuilder progress

- Checkpoint 293 compiles EnableDynamicsCollide. Both EA assets matched. All 286 tests and Clippy passed.
- Checkpoint 292 compiles EjectPassengersSpecialAbilityUpdate, including ejection bone offsets, animation, and inherited fields. All five EA assets matched. All 285 tests and Clippy passed.
- Checkpoints 288–291 link concrete render and audio assets, compile assisted placement hints, and resolve inherited references.
- Checkpoints 274–287 add locomotors, surface mappings, GameObject fields and behaviors, button variants, FX inheritance, and weapon damage arcs.

- Surge's release SDK commands compiled AptUI, globals, and all static XML, fixed 3,911 static assets, created two LoD streams, and packed the streams BIG in 16.50 seconds. Next tasks are native merging of 194 precompiled animation overrides and verification of the Studio build path.
