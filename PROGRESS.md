# Rust BinaryAssetBuilder progress

- Checkpoint 307 compiles ToppleUpdate effects, stump IDs, physics settings, and flags. All six EA assets matched. All 294 tests and Clippy passed. Surge's official map reaches reference resolution.
- Checkpoint 306 compiles PropDraw and its optional model reference. All four EA assets matched. All 294 tests and Clippy passed. Surge's official map reaches ToppleUpdate.
- Checkpoint 305 compiles all four BuildingBehavior window names, including explicitly empty values. Both EA fixtures matched. All 294 tests and Clippy passed.
- Checkpoint 304 compiles ContestableGarrisonContain through the shared garrison encoder. All three EA assets matched. All 294 tests and Clippy passed. Surge's official map reaches BuildingBehavior window fields.
- Checkpoint 303 compiles inherited Body damage ranges, including thresholds and modifier references. All four EA assets matched. All 294 tests and Clippy passed.
- Checkpoint 302 compiles GameObject Browser category lists. All three EA fixtures matched. All 294 tests and Clippy passed.
- Checkpoint 301 compiles BridgeBuilder references, lengths, optional pieces, and damage threshold. All five EA assets matched. All 294 tests and Clippy passed. Surge's official map reaches the GameObject Browser field.
- Checkpoint 300 adds HighlanderBody, ManagedActiveBody, ManagedImmortalBody, and ManagerBody using the shared ActiveBody encoder. All eight EA fixtures matched. All 294 tests and Clippy passed. Surge's official map reaches BridgeBuilder.
- Checkpoint 299 compiles TerrainTextureAtlas tile packing, wrapped borders, and DDS mip chains. Three EA fixtures matched tile layout, DDS headers, and level-zero normal bytes. Compression, some mip bytes, and instance hashes differ. All 294 tests and Clippy passed.
- Checkpoint 298 compiles GameMap objects, world properties, and water analysis. All five EA assets matched. All 293 tests and Clippy passed.
- Checkpoint 297 compiles Road geometry settings and texture references. Both EA assets matched. All 292 tests and Clippy passed.
- Checkpoint 296 compiles GameScriptList, including nested groups, object parameters, and ANSI action hashes. All six EA assets matched. All 291 tests and Clippy passed.
- Checkpoint 295 adds native ModStudio Step 7 for precompiled W3D animations. All 194 Surge overrides produced byte-identical manifest, bin, relo, and imp files against EA. All 290 tests and Clippy passed.
- Checkpoints 288–294 link concrete render and audio assets, fix inherited references, and add assisted placement, passenger ejection, dynamics collision, and stance templates.
- The generated ModStudio commands ran Surge Steps 4–8 and 10–11 in 18.44 seconds using Rust. The build linked 3,911 static assets, wrote two LoD streams, and packaged both BIGs and the skudef.
- Next, extend native coverage beyond Surge's active settings. Step 9 still needs the official map compiler.
