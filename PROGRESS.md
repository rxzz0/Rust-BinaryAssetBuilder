# Rust BinaryAssetBuilder progress

- Checkpoint 237 encodes TrackMarks, WadingParticleSys, MinLODRequired, and ProjectileBoneFeedbackEnabledSlots in the shared ScriptedModelDraw layout. Both draw fixtures match EA byte for byte. All 230 tests and Clippy passed.
- Checkpoint 236 encodes ParalyzeNugget, AttributeModifierNugget, and shared weapon effect conditions. Checkpoint 235 encodes WeaponTemplate root attributes.
- Surge Step 6 passes the Hammer Tank's TruckDraw track marks and reaches SkirmishAiInfo OnlyUseWhenHaveTarget. Next task is verifying the special-power AI settings layout.
