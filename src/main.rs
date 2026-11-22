#![forbid(unsafe_code)]

use binary_asset_builder::big::stage_big;
use binary_asset_builder::compile::{
    CompiledStream, compile_apt_ui, compile_apt_ui_directory, compile_map, compile_sdk_global,
    compile_sdk_map_overrides, compile_sdk_static, compile_static, merge_assets, sdk_coverage,
    verify_ea_batch, write_compiled_stream,
};
use binary_asset_builder::fix_assets::{fix_assets, fix_assets_allow_missing};
use binary_asset_builder::linked_stream::{check_linked_files, link_assets};
use binary_asset_builder::manifest::{ByteOrder, Manifest};
use binary_asset_builder::sdk_references::prepare_references;
use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
Usage
  binary-asset-builder compile <file.xml> --data-root <directory> --output <new-directory>
  binary-asset-builder compile-aptui <input-directory> --data-root <directory> --output <directory>
  binary-asset-builder compile-static <file.xml> --data-root <directory> --references <directory> --output <new-directory>
  binary-asset-builder compile-map <file.xml> --data-root <directory> --references <directory> --output <new-directory>
  binary-asset-builder compile-maps-sdk <maps-directory> --sdk-root <directory> --references <builtmods-directory> --output <maps-directory> [--game-data <directory>]
  binary-asset-builder compile-static-sdk <file.xml> --sdk-root <directory> --output <builtmods-directory> [--game-data <directory>]
  binary-asset-builder compile-global-sdk <additionalmaps-directory> --sdk-root <directory> --output <builtmods-directory> [--game-data <directory>]
  binary-asset-builder merge-assets <file.manifest> --assets <directory>
  binary-asset-builder fix-assets <mod.manifest> --references <builtmods-directory> [--allow-missing-references]
  binary-asset-builder pack-big <directory> --output <file.big> [--reuse <previous.big>]
  binary-asset-builder relink <file.manifest> --assets <directory> --output <new-directory>
  binary-asset-builder sdk-coverage <schema-directory>
  binary-asset-builder verify-ea-batch <file.xml> <ea.manifest> <ea-assets-directory>
  binary-asset-builder inspect <file.manifest>
  binary-asset-builder check-linked <file.manifest>

Set BAB_NO_WARNINGS=1 to hide warnings. Build errors remain visible.";

fn diagnostic(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" || flag == "/?" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [command, schema] if command == "sdk-coverage" => {
            diagnostic(sdk_coverage(Path::new(schema), io::stdout().lock()))
        }
        [command, input, manifest, assets] if command == "verify-ea-batch" => {
            diagnostic(verify_ea_batch(
                Path::new(input),
                Path::new(manifest),
                Path::new(assets),
                io::stdout().lock(),
            ))
        }
        [command, input, flag, assets] if command == "merge-assets" && flag == "--assets" => {
            let input = Path::new(input);
            diagnostic(merge_assets(input, Path::new(assets)).map(|count| {
                println!("Merged {count} precompiled assets into {}", input.display());
            }))
        }
        [command, input, flag, references, options @ ..]
            if command == "fix-assets"
                && flag == "--references"
                && (options.is_empty()
                    || matches!(options, [option] if option == "--allow-missing-references")) =>
        {
            let input = Path::new(input);
            let fix = if options.is_empty() {
                fix_assets
            } else {
                fix_assets_allow_missing
            };
            match fix(input, Path::new(references)) {
                Ok(result) => {
                    println!(
                        "Verified {} assets, reconciled {} hashes, and built 2 LoD streams",
                        result.assets, result.hashes_fixed
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{} ({error})", input.display());
                    ExitCode::FAILURE
                }
            }
        }
        [command, input, flag, output, options @ ..]
            if command == "pack-big"
                && flag == "--output"
                && (options.is_empty()
                    || matches!(options, [option, _] if option == "--reuse")) =>
        {
            let previous = options.get(1).unwrap_or(output);
            let start = std::time::Instant::now();
            match stage_big(Path::new(input), Path::new(output), Path::new(previous)) {
                Ok(result) => {
                    let action = if result.reused {
                        "Verified existing BIG with"
                    } else {
                        "Packed"
                    };
                    println!(
                        "{action} {} files at {} in {:.2}s",
                        result.entries,
                        Path::new(output).display(),
                        start.elapsed().as_secs_f64()
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{} ({error})", Path::new(output).display());
                    ExitCode::FAILURE
                }
            }
        }
        [command, input, assets_flag, assets, output_flag, output]
            if command == "relink" && assets_flag == "--assets" && output_flag == "--output" =>
        {
            let input = Path::new(input);
            match relink(input, Path::new(assets), Path::new(output)) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{} ({error})", input.display());
                    ExitCode::FAILURE
                }
            }
        }
        [
            command,
            input,
            sdk_flag,
            sdk,
            output_flag,
            output,
            options @ ..,
        ] if (command == "compile-static-sdk" || command == "compile-global-sdk")
            && sdk_flag == "--sdk-root"
            && output_flag == "--output"
            && (options.is_empty() || matches!(options, [flag, _] if flag == "--game-data")) =>
        {
            let input = Path::new(input);
            let sdk = Path::new(sdk);
            let output = Path::new(output);
            if let [_, game_data] = options {
                match prepare_references(output, Path::new(game_data)) {
                    Ok(0) => {}
                    Ok(count) => {
                        println!("Prepared {count} stock reference manifests from Red Alert 3")
                    }
                    Err(error) => return diagnostic(Err(error)),
                }
            }
            let result = if command == "compile-global-sdk" {
                compile_sdk_global(input, sdk, output)
            } else {
                compile_sdk_static(input, sdk, output)
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{} ({error})", input.display());
                    ExitCode::FAILURE
                }
            }
        }
        [
            command,
            input,
            sdk_flag,
            sdk,
            references_flag,
            references,
            output_flag,
            output,
            options @ ..,
        ] if command == "compile-maps-sdk"
            && sdk_flag == "--sdk-root"
            && references_flag == "--references"
            && output_flag == "--output"
            && (options.is_empty() || matches!(options, [flag, _] if flag == "--game-data")) =>
        {
            diagnostic(
                compile_sdk_map_overrides(
                    Path::new(input),
                    Path::new(sdk),
                    Path::new(references),
                    Path::new(output),
                    options.get(1).map(Path::new),
                )
                .map(|count| println!("Compiled {count} map override streams with Rust")),
            )
        }
        [
            command,
            input,
            root_flag,
            root,
            references_flag,
            references,
            output_flag,
            output,
        ] if (command == "compile-static" || command == "compile-map")
            && root_flag == "--data-root"
            && references_flag == "--references"
            && output_flag == "--output" =>
        {
            let input = Path::new(input);
            let compile = if command == "compile-map" {
                compile_map
            } else {
                compile_static
            };
            let result = compile(input, Path::new(root), Path::new(references))
                .and_then(|stream| write_compiled_stream(input, Path::new(output), stream));
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{} ({error})", input.display());
                    ExitCode::FAILURE
                }
            }
        }
        [command, input, root_flag, root, output_flag, output]
            if (command == "compile" || command == "compile-aptui")
                && root_flag == "--data-root"
                && output_flag == "--output" =>
        {
            let input = Path::new(input);
            let root = Path::new(root);
            let output = Path::new(output);
            let result = if command == "compile-aptui" {
                compile_apt_ui_directory(input, root, output)
            } else {
                compile_apt_ui(input, root)
                    .and_then(|stream| write_compiled_stream(input, output, stream))
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{} ({error})", input.display());
                    ExitCode::FAILURE
                }
            }
        }
        [command, path] if command == "inspect" || command == "check-linked" => {
            let path = Path::new(path);
            match run(path, command == "check-linked") {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("{} ({error})", path.display());
                    ExitCode::FAILURE
                }
            }
        }
        [input, ..]
            if Path::new(input).extension().is_some_and(|extension| {
                extension.eq_ignore_ascii_case("xml") || extension.eq_ignore_ascii_case("babproj")
            }) =>
        {
            eprintln!(
                "Use compile for AptUI XML or compile-static for supported static XML. See --help for {}.",
                Path::new(input).display()
            );
            ExitCode::from(2)
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn relink(input: &Path, assets: &Path, output: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(input)?;
    let manifest = Manifest::parse(&bytes)?;
    let files = link_assets(&manifest, |name| fs::read(assets.join(name)))?;
    write_compiled_stream(
        input,
        output,
        CompiledStream {
            manifest: bytes,
            files,
            custom_data: Vec::new(),
        },
    )?;
    Ok(())
}

fn run(path: &Path, check_linked: bool) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let manifest = Manifest::parse(&bytes)?;
    let mut stdout = io::stdout().lock();
    if check_linked {
        check_linked_files(path, &manifest)?;
        writeln!(stdout, "Linked file lengths and checksum headers match")?;
        return Ok(());
    }

    let order = match manifest.byte_order {
        ByteOrder::Little => "little-endian",
        ByteOrder::Big => "big-endian",
    };
    let stream = if manifest.is_linked {
        "linked"
    } else {
        "unlinked"
    };
    writeln!(
        stdout,
        "Version {} {order} {stream} manifest",
        manifest.version
    )?;
    writeln!(stdout, "Stream checksum {:08x}", manifest.stream_checksum)?;
    writeln!(stdout, "Assets {}", manifest.assets.len())?;
    for asset in &manifest.assets {
        writeln!(
            stdout,
            "  {:08x}:{:08x} {}",
            asset.id.type_id,
            asset.id.instance_id,
            String::from_utf8_lossy(asset.name)
        )?;
    }
    writeln!(
        stdout,
        "Referenced manifests {}",
        manifest.referenced_manifests.len()
    )?;
    for reference in &manifest.referenced_manifests {
        let kind = if reference.is_patch {
            "patch base"
        } else {
            "reference"
        };
        writeln!(
            stdout,
            "  {kind} {}",
            String::from_utf8_lossy(reference.path)
        )?;
    }
    Ok(())
}
