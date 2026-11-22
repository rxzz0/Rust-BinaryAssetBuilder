# BinaryAssetBuilder
BinaryAssetBuilder implementation for Kane's Wrath in .Net

### Implemented Asset Types
* [ ] TestGameObject
* [ ] TestTexture
* [ ] TestTextureCollection
* [x] WeaponTemplate
* [x] LocomotorTemplate
* [ ] GameObject                                                0x132408DB
* [ ] FXParticleSystemTemplate                                  0xA148D511
* [ ] Weather                                                   0x368A8BA2
* [ ] ShadowMap                                                 0xC6389FA6
* [ ] WaterTransparency                                         0x331DA6CE
* [ ] Texture
* [ ] OnDemandTexture
* [ ] W3DMesh                                                   0xC9D7E778
* [ ] W3DContainer                                              0x909DD93F
* [ ] W3DHierarchy                                              0x3BC26A7A
* [ ] W3DAnimation                                              0xCC069193
* [ ] W3DCollisionBox                                           0xC917E725
* [ ] ArmyDefinition                                            0x57213EA5
* [ ] AIPersonalityDefinition                                   0x7DCE182F
* [ ] FXList                                                    0xEBE8A8A4
* [ ] ObjectCreationList                                        0x683D4DE5
* [ ] ObjectFilterAsset                                         0x25970AF7
* [ ] SpecialPowerTemplate                                      0x5EF0ACA9
* [ ] UpgradeTemplate                                           0x1E53F384
* [ ] SkirmishOpeningMove                                       0x21EE29FA
* [ ] AIStateDefinition                                         0x262BE85F
* [ ] AIStrategicStateDefinition                                0x1E27DA26
* [ ] AIBudgetStateDefinition                                   0xA10F9630
* [ ] AITargetingHeuristic                                      0xB7A2C222
* [ ] GameMap                                                   0x3EC9C79B
* [x] AttributeModifier
* [x] ArmorTemplate
* [ ] MissionTemplate                                           0x0D283295
* [ ] TheaterOfWarTemplate                                      0xE60C9724
* [ ] CampaignTemplate                                          0xAC60B530
* [ ] RadiusCursorLibrary                                       0xD62B490F
* [ ] AudioFile                                                 0x46410F77
* [ ] AudioEvent                                                0x1B886049
* [ ] MusicTrack                                                0x1469548A
* [ ] DialogEvent                                               0x8655CDB4
* [ ] AmbientStream                                             0xDABB1C4B
* [ ] Multisound                                                0x12B1C67C
* [ ] MusicPalette                                              0x6A7AF822
* [ ] MusicScriptConditionNugget_LocalPlayerIsObserver          0xAFB6AF3A
* [ ] MusicScriptConditionNugget_UnitsFarFromBase               0xD889BF98
* [ ] MusicScriptConditionNugget_TimeFromStartOfLevel           0xAA4A9E23
* [ ] MusicScriptConditionNugget_TrackPlayedCount               0x4FCFFAB1
* [ ] MusicScriptConditionNugget_SpecificTrackTypePlaying       0xBCAD9B77
* [ ] MusicScriptConditionNugget_AnyTrackPlaying                0x337BC326
* [ ] MusicScriptConditionNugget_ObjectsOfTypeExist             0x9586411C
* [ ] MusicScriptConditionNugget_EvaEventPlayedRecently         0x1F200F13
* [ ] MusicScriptConditionNugget_ObjectsNearEvaEvent            0x0EC4D160
* [ ] MusicScriptConditionNugget_ScoredKillCount                0x5C0F93DC
* [ ] MusicScriptConditionNugget_Not                            0xB886383B
* [ ] MusicScriptConditionNugget_Or                             0x81114695
* [ ] MusicScriptConditionNugget_And                            0x10173347
* [ ] MusicScriptTrack                                          0x702C8407
* [ ] LocalBuildListMonitor                                     0x99CC030A
* [ ] MpGameRules                                               0xEDDBB607
* [ ] ExperienceLevelTemplate                                   0xAE55047B
* [ ] MissionObjectiveList                                      0xC385A8C1
* [ ] StringHashTable                                           0x2C112832
* [ ] InGameUISettings                                          0x49FE3760
* [ ] DamageFX                                                  0x4DF81EBD
* [ ] MultiplayerSettings                                       0x1BAF4C42
* [ ] OnlineChatColors                                          0xF3645AA7
* [ ] MultiplayerColor                                          0x966F336A
* [ ] GameLODPreset                                             0x19DAC24D
* [ ] StaticGameLOD                                             0xBEAF1CC9
* [ ] DynamicGameLOD                                            0x71BAD792
* [ ] AudioLOD                                                  0x3ABBF00F
* [ ] VideoEventList                                            0x999FCBE3
* [ ] UIConfigList                                              0xB3B7607A
* [ ] PackedTextureImage                                        0x2FAEB748
* [ ] OnDemandTextureImage                                      0xF3F4AEEC
* [ ] TerrainTextureAtlas
* [ ] Mouse                                                     0x73FE99B0
* [ ] Achievement                                               0xC8D16E6D
* [ ] StanceTemplate                                            0x5C6E0E41
* [ ] TargetingCompareList                                      0x57CA5C81
* [ ] TargetingDistanceCompare                                  0xED45F096
* [ ] TargetingCombatChainCompare                               0x553808EF
* [ ] TargetingInTurretArcCompare                               0xCD24391A
* [ ] Road                                                      0xDCF3C28B
* [ ] Environment                                               0x878C42E0
* [ ] LogicCommand                                              0x97D0A46E
* [ ] LogicCommandSet                                           0x6D148BD7
* [ ] MiscAudio                                                 0xFA4817E2
* [ ] AudioSettings                                             0x89AA7DDE
* [ ] CrowdResponse                                             0x66FB33A0
* [ ] MapMetaData                                               0x59013A51
* [ ] LargeGroupAudioMap                                        0x9CBC0553
* [ ] AptAptData                                                0x36866072
* [ ] AptConstData                                              0x1CE8E595
* [ ] AptDatData                                                0x3BF7FEB9
* [ ] AptGeometryData                                           0x58F89E8B
* [ ] MappableKey                                               0xE005A668
* [ ] HotKeySlot                                                0x1AC54E60
* [ ] DefaultHotKeys                                            0x0E12479D
* [ ] InGameUIGroupSelectionCommandSlots                        0xF6CE1A68
* [ ] InGameUILookAtCommandSlots                                0x8F9F9918
* [ ] InGameUITacticalCommandSlots                              0xC24AEFF1
* [ ] InGameUIVoiceChatCommandSlots                             0x3592E352
* [ ] InGameUISideBarCommandSlots                               0xAF956455
* [ ] InGameUIPlayerPowerCommandSlots                           0x4AB425C6
* [ ] InGameUIUnitAbilityCommandSlots                           0x9DAA4182
* [ ] GameScriptList                                            0x5AC6FA18
* [ ] IntelDB                                                   0xFBB64F90
* [ ] BootupDisplaySequence                                     0x84C1C2F0
* [ ] UnitTypeIcon                                              0xF7AB74BE
* [ ] ImageSequence                                             0x217CF953
* [ ] UnitOverlayIconSettings                                   0xDFC78E66
* [ ] TheVersion                                                0xF659EF49
* [ ] DLContent                                                 0x4E1A5713
* [ ] PhaseEffect                                               0x4877D566
* [ ] ConnectionLineManager                                     0x7AEB73B2
* [ ] InGameUIFixedElementHotKeySlotMap                         0x475EA260

### Tiberium Wars Only Types
* [ ] AudioFileMP3Passthrough                                   0x610DB321
* [ ] MP3MusicTrack
* [ ] MP3DialogEvent
* [ ] MP3AmbientStream
* [ ] UnitAbilityButtonTemplateStore                            0x5A48D289
* [ ] PlayerPowerButtonTemplateStore                            0xDB57AB4F
* [ ] CommandSet                                                0x3CFF78A1

### Kane's Wrath Only Types
* [ ] UnitAbilityButtonTemplate
* [ ] PlayerPowerButtonTemplate
* [ ] StrikeForceBuildTemplate
* [ ] MetagameOperationsInfoType
* [ ] MetaGameUITacticalCommandSlots
* [ ] MetaGameUICommonOpCommandSlots
* [ ] MetaGameMapZoneData
* [ ] MetaGameStaticData
* [ ] ButtonSingleStateData
* [ ] JoypadCommandBarTemplate
* [ ] JoypadCommandBarButtonTemplate
* [ ] UIJoypadCommandBarButtonBuild
* [ ] UIJoypadCommandBarHomogenousGroup
* [ ] UIJoypadCommandBarMixedGroup
* [ ] UIJoypadCommandBarSingleUnit
* [ ] UIJoypadCommandBarStances
* [ ] UIJoypadCommandBarTopMenu
* [ ] UIJoypadCommandBarMgTopMenu

### Install in RA3 ModStudio

Close ModStudio and run the PowerShell installer with the path to the SDK folder that contains `EALAModStudio.exe`.

```powershell
.\sdk\Start-Ra3ModStudio.ps1 -SdkRoot 'X:\RA3 MOD SDK' -InstallDefault
```

The installer activates the bundled script and saves the original at `defaultscript\defaultscript.cs` inside the SDK. Reinstalling preserves that backup. Existing build script customizations are stored in the backup. To restore them, close ModStudio and copy the backed-up file to the SDK root as `defaultscript.cs`.

After switching from an older script, review the **Create BIG Files** and **Create Skudef File** checkboxes before the first build.

### Build map overrides

In ModStudio, select **Build Map Overrides** with the Rust compiler. Put each source map in a direct child folder of `Mods/<mod>/Misc/data/maps` with its `map.xml` and override XML files. The compiler writes the linked `map_mod` stream and `map.version` beside the map. **Create BIG Files** packages these files in the Misc BIG. Keep archived sources in a subfolder such as `INGORE`. The `Official` directory is excluded from the ModStudio build step.

### RA3 SDK coverage and EA verification

Run these from the repository root after building the Rust release.

~~~powershell
cargo build --release --locked
$compiler = 'target/release/binary-asset-builder.exe'
$sdk = 'X:\RA3 MOD SDK'
& $compiler sdk-coverage (Join-Path $sdk 'Schemas/xsd') |
    Set-Content -Encoding utf8 sdk-coverage.tsv
& ./sdk/Test-EaBatch.ps1 -SdkRoot $sdk -RustCompiler $compiler -InputXml tests/fixtures/ea-batches/scripted-model-draw.xml -OutputDirectory .scratch/ea-draw-run
~~~

The TSV inventories the SDK's top-level declarations and the five GameObject module groups. Asset status covers native static dispatch. Module status records the currently implemented dispatch names. An encoder entry does not imply full support. ScriptedModelDraw also reports attribute and child coverage.

Use a new output directory for each EA run. A batch is a self-contained AssetDeclaration with independent probes and no includes or inheritance. The comparator matches each asset by ID and checks binary payload, relocations, imports, ordered references, and metadata. The checked-in pilot oracle also runs under cargo test without EA installed. EA is used only to generate verification data.


### Compare complete EA and Rust builds

Build the Rust release, then run the shared compiler probes with the RA3 SDK installed.

```powershell
cargo build --release
.\sdk\Test-EaConformance.ps1 -SdkRoot 'X:\RA3 MOD SDK' -RustCompiler .\target\release\binary-asset-builder.exe -OutputDirectory .\.scratch\ea-conformance
```

Give each run a new output directory. The script copies the probe XML and SDK schemas, builds both compilers with isolated outputs, and preserves logs and streams by case. It reports build failures, differences in unknown-reference warnings, missing streams, version sidecars, and per-asset linked differences. It exits with status 1 when it finds differences. EA is used only by this test script.

Run `binary-asset-builder compare-linked <ea.manifest> <rust.manifest>` to compare an existing pair of linked streams. It validates the three binary files against each manifest and reports differences in stream order, references, checksums, asset metadata, and payload, relocation, and import bytes.
