# Rust BinaryAssetBuilder progress

- Checkpoint 243 encodes OpenContain and TransportContain, including their shared passenger filters, modifiers, and member status entries, plus transport payloads, triggers, and timing. The fixture matches EA byte for byte. All 234 tests and Clippy passed.
- Checkpoint 242 encodes AudioLoopUpgrade. Checkpoint 241 encodes death-weapon probabilities and offsets. Checkpoint 240 encodes object-creation upgrades and the slave watcher.
- Surge Step 6 passes transport containment and reaches the Multigunner IFV's UpgradeSoundSelectorClientBehavior. Next task is extending the shared client audio selector encoder.
