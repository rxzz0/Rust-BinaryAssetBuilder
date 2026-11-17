# Rust BinaryAssetBuilder progress

- Checkpoint 221 adds native `PlayerTemplate` encoding for faction data, starting units, resources, beacons, and tech bonuses. Three EA fixtures match byte for byte. All 216 tests and Clippy passed.
- Checkpoint 220 applies EA include precedence to static assets. Checkpoint 219 adds native AI special power data in logic commands.
- Surge Step 6 reaches `UpgradeTemplate:Upgrade_AlliedTech2` inheriting from `BasePurchasableUpgrade`. Next task is to route upgrade inheritance through the existing schema-based XML merger and compare with EA.
