# Rust BinaryAssetBuilder progress

- Checkpoint 220 applies EA include precedence to static assets. The first sibling include wins, the parent document takes precedence, and duplicates within one document are rejected. All 215 tests and Clippy passed.
- Checkpoint 219 adds native `AISpecialPowerInfo` and `SpecialPowerWeapon` inside `LogicCommand`. Three EA fixtures match byte for byte.
- Surge Step 6 now reaches `PlayerTemplate` in `Data/Mod/Common/PlayerTemplates.xml`. Next task is to probe and implement the native encoder.
