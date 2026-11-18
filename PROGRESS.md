# Rust BinaryAssetBuilder progress

- Checkpoint 289 adds AssistedPlacementInfoBehavior, including hint objects, optional filter references, and placement restrictions. Both fixtures match EA. All 282 tests and Clippy passed.
- Checkpoint 288 links abstract render references to concrete local and external W3D assets. Checkpoints 284–287 add button variants, inherited FX lists, and weapon damage arcs.
- Checkpoints 274–283 add locomotor and surface mappings, transformed display and upgrade fields, sweeping damage, animation sound support, collision, ore, JetAIUpdate, and cleared EVA events.

- Surge release Step 6 reached an inherited StanceTemplate reference on AlliedAntiAirVehicleTech1. Reference indexing appends an ordinal that the stance encoder was hashing into the asset name. Resolve that reference next.
