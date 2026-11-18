# Rust BinaryAssetBuilder progress

- Checkpoint 255 adds native FXList encoding with five nugget types, including ParticleSystem, plus common filters, model conditions and weather rules. All three EA assets match byte for byte. All 246 tests and Clippy passed.
- Checkpoint 254 encodes AttachNugget, StripMaxHealthPercentNugget and InformationWarfareNugget.

- Surge Step 6 passes the Tesla Trooper's FXList and reaches ObjectSpecificEntry in AudioArrayVoice. Next tasks are the remaining FXList variants and the audio entry encoder.
