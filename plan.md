# Parallel build plan

Parallelize the Rust BinaryAssetBuilder on the `parallel` branch while preserving the streams the game already accepts. Aim for a median clean Surge build of **5 seconds or less** on the same PC and SDK drive. Use phase timings to decide which work to parallelize and which work to leave sequential.

## Scope and baseline

Use the existing SDK commands and EALAModStudio workflow, with Rust selected and official map compilation skipped. A clean run starts in a fresh output directory with the stock SDK reference manifests and compiles the same Surge inputs through AptUI, global data, static data, merge, Fix Assets, both BIG archives, and Skudef creation. The Rust path remains native. EA fixtures and the current sequential Rust build provide separate compatibility checks.

| Stage | Measured time |
| --- | ---: |
| AptUI | 0.12 s |
| Global data | 1.70 s |
| Static data | 9.51 s |
| Merge and Fix Assets | 0.33 s |
| Streams BIG | 0.36 s |
| Misc BIG | 3.91 s |
| Total, including Skudef and overhead | **15.95 s** |

This was one command-line run at checkpoint 340. Fresh build output and cold Windows filesystem cache are separate conditions. The comparison should use the same storage and warning settings and report cold-cache results separately if measured.

## Repeated baseline at checkpoint 342

Five clean builds with the installed SDK Rust compiler produced a median of **16.70 s** and a range of **15.88–17.71 s**. Static data took a median of **9.62 s** of wall time and **8.97 s** of process CPU time. Global data took **1.92 s**, and Misc packaging took **4.05 s**. The runs used fresh output directories on `X:`, the same four stock reference manifests, hidden warnings, and the existing Surge sources. Windows filesystem caches were left in their normal state. The host exposed 12 logical processors.

Two additional runs used a temporary instrumented copy of the same Rust source. The table reports the range across those runs. Asset timings include per-asset validation and source reads where applicable.

| Profile measurement | Time |
| --- | ---: |
| W3DAnimation compilation, 194 assets | 3.03–3.06 s |
| AudioFile compilation, 1,289 assets | 2.32–2.33 s |
| W3DMesh compilation, 134 assets | 0.68–0.71 s |
| Static traversal apart from timed asset compilation | 1.59–1.70 s |
| Static stream publication | 0.94–1.15 s |
| Misc archive buffered writes, 1.09 GB archive | 2.82–2.93 s |
| Misc archive source reads | 0.18–0.19 s |

The static process used approximately **163 MiB** peak working set in the instrumented runs. All seven runs passed linked checks and had identical SHA-256 hashes for **1,358 output files**, including both archives and Skudef. Local run logs and reference hashes are under `.scratch/parallel-baseline`.

Prepare independent W3DAnimation and AudioFile jobs first. Together they accounted for roughly **5.4 s** across both profiles. When testing packaging, measure whether overlapping the Misc archive write with compilation helps on this drive.

## Static encoding at checkpoint 344

Five alternating pairs of clean Surge builds used the same release executable. Set `BAB_ASSET_WORKERS=4` to enable workers for SDK static AudioFile and W3DAnimation encoding. The default and `BAB_ASSET_WORKERS=1` use the serial path.

| Workers | Median full build (range) | Median static data (range) | Median static CPU time | Median static sampled peak memory |
| --- | ---: | ---: | ---: | ---: |
| 1 | 16.50 s (15.88–16.57 s) | 9.68 s (9.51–9.92 s) | 9.02 s | 161 MiB |
| 4 | 12.66 s (12.53–12.92 s) | 5.68 s (5.64–5.76 s) | 9.77 s | 165 MiB |

Each build passed its linked check and matched the 1,358 reference files byte for byte. A separate warning-enabled Surge compile produced the same 2,381 warning lines with both worker settings. The first-error regression test compares both settings. On failed builds, schema warnings can appear for declarations prepared before an earlier worker failure is collected.

## Implementation checkpoints

- [x] **1. Capture a repeatable baseline and profile.** Freeze the compiler commit, Surge inputs, stock manifests, build settings, and output drive for paired runs. Time at least five clean runs per configuration, alternate configurations to limit filesystem cache bias, and record median, range, peak memory, CPU use, and per-phase timings. Within static data, measure include and schema preparation, per-type encoding, linking, and output writes. Within packaging, separate enumeration, source reads, and archive writes. Save file counts and output hashes from the sequential build. Run warning-enabled checks separately from timed runs.
- [x] **2. Separate preparation from encoding in `src/compile.rs` using one worker.** Keep traversal, include and define expansion, inheritance context, and duplicate handling in their current order. Give each eligible declaration a submission index, source path, and owned or safely shared input. Retain document-level parsing where possible to avoid reparsing an entire document per asset. Make the smallest read-only context needed by the selected encoders available to workers. Audit the current `Rc` data before sharing anything across threads. Keep music header caching, inherited assets, tentative instance discovery, and final reference resolution on their existing ordered path until measurements justify changing them. Verify sequential parity before adding workers.
- [x] **3. Parallelize the measured hot encoders with bounded workers.** Start with independent `W3DAnimation` and `AudioFile` work, then evaluate `W3DMesh` separately. Use Rust standard-library threads joined at the end of the compile and bound the number of in-flight jobs and results. Keep one worker available as a reference path and size higher worker counts from measured CPU and drive behavior. Collect each result and its diagnostics by submission index. Commit results in the original traversal order so override precedence, manifest ordering, first reported errors, and warning output stay predictable. Workers return asset data and leave publication to the existing coordinator.
- [ ] **4. Parallelize independent AptUI and global metadata streams in `src/compile_batch.rs` and `src/compile/sdk_static.rs`.** Check duplicate output names and what stock or generated references each stream reads. Run independent streams within the worker budget, collect results in source order, and keep the current batch publication and rollback paths. Measure before sharing schema loading across streams. If a stream already uses internal workers, give it a slice of the same budget or run it with one worker to avoid oversubscribing the machine.
- [ ] **5. Shorten the packaging critical path.** Profile `src/big.rs` on the SDK drive. Measure concurrent Streams and Misc archive creation first. Also measure whether starting Misc earlier helps, since its inputs can be checked independently once the build paths are ready. Deliver Skudef and final package files only when the required compilation and both archive writes succeed. If the drive benefits from multiple readers, test bounded read-ahead with one ordered writer. Keep BIG entry ordering, source-change detection, output size checks, and temporary-file publication. Compare I/O changes to the sequential output and keep only repeatable gains.
- [ ] **6. Re-measure and optimize the remaining hotspot.** Sweep worker budgets such as 1, 2, 4, and the machine’s available parallelism. Check the combined workload rather than adding timings from isolated stages. Use the final profile to assess repeated schema or manifest loading, path resolution, parsing, or redundant file copies. Measure any incremental-build cache separately from the clean-build target. If five seconds remains out of reach, record the best verified time and the phase that sets the limit.
- [ ] **7. Verify correctness and failure behavior.** Compare one-worker and multi-worker output byte for byte across repeated fresh runs, including manifests, bins, relocations, imports, custom data, LoD streams, both BIGs, and Skudef. Compare with checkpoint 340 using identical inputs and build settings. Run EA fixtures, all Rust tests, strict Clippy, and the linked stream checks. Exercise an empty mod, different include and inheritance patterns, duplicate IDs, unresolved references, schema errors, missing files, and packaging input changes. Verify that failure leaves the previous published output usable and that warnings remain readable and suppressible.
- [ ] **8. Verify ModStudio and choose rollout settings.** Time a clean Rust build through the existing EALAModStudio window and confirm the EA selector still runs the original workflow. Smoke-test Surge in game, including completion of Archer Maiden or Cryo Legionnaire. Pick the default worker limit from benchmark and compatibility results and retain the one-worker setting for troubleshooting. Save the final timing table and exact reproduction steps in `PROGRESS.md`.

## Acceptance and checkpoint policy

- Use new output directories with identical source inputs for comparisons. Run the baseline and candidate on the same PC and drive. Report which stages, settings, and tools the timing covers.
- Require repeatable byte-for-byte parity between Rust worker settings. Use existing EA fixtures for differences that the project already tracks independently of parallelism.
- Run `cargo fmt --check`, `cargo test --all-targets`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` before rollout. Verify the test package with `check-linked` and exercise ModStudio and the game as separate checks.
- Commit each independently verified step on `parallel`. Record the commit, timing, parity result, and next step in `PROGRESS.md`. Keep the experiment on `parallel` until the correctness and performance results are reviewed.

## Questions to answer with the baseline

- How much of the 9.51 seconds in static data goes to audio, mesh and texture encoding, XML and schema work, linking, and writes?
- Is Misc packaging limited by opening many files, transferring bytes on the SDK drive, or writing the archive?
