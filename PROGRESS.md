# Rust BinaryAssetBuilder progress

- Checkpoint 222 adds native `UpgradeTemplate` inheritance through the schema-based XML merger. The base and derived payloads, relocations, and instance hashes match EA. All 217 tests and Clippy passed.
- Checkpoint 221 adds native `PlayerTemplate` encoding. Checkpoint 220 matches EA static include precedence. Checkpoint 219 adds AI special power data in logic commands.
- Surge Step 6 reaches `AttributeModifier` in `Data/Mod/Units/MP/Allied Engineer/AttributeModifier.xml`. Next task is native `AttributeModifier` encoding.
