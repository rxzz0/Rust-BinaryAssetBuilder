# Rust BinaryAssetBuilder progress

- Checkpoint 252 encodes TrackIncomingWeaponFireBehavior and TrackDamage, including the tracking interval and instant-weapon filter. The fixture matches EA byte for byte. All 243 tests and Clippy passed.
- Checkpoint 251 encodes SpecialAbilityUpdate and TimedOpacityUpdate. Checkpoint 250 adds module activation and toggle triggers.
- Surge Step 6 passes Shinobi's GameObject and reaches VeterancyProjectiles in its weapon template. Next task is completing ProjectileNugget child fields.
