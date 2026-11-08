#![forbid(unsafe_code)]

use binary_asset_builder::compile::{
    CompiledStream, compile_apt_ui, compile_sdk_static, compile_static,
};
use binary_asset_builder::linked_stream::check_linked_files;
use binary_asset_builder::manifest::{ByteOrder, Manifest};
use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

mod compile_batch;

const USAGE: &str = "\
Usage
  binary-asset-builder compile <file.xml> --data-root <directory> --output <new-directory>
  binary-asset-builder compile-aptui <input-directory> --data-root <directory> --output <directory>
  binary-asset-builder compile-static <file.xml> --data-root <directory> --references <directory> --output <new-directory>
  binary-asset-builder compile-static-sdk <file.xml> --sdk-root <directory> --output <builtmods-directory>
  binary-asset-builder inspect <file.manifest>
  binary-asset-builder check-linked <file.manifest>";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" || flag == "/?" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [command, input, sdk_flag, sdk, output_flag, output]
            if command == "compile-static-sdk"
                && sdk_flag == "--sdk-root"
                && output_flag == "--output" =>
        {
            let input = Path::new(input);
            match compile_sdk_static(input, Path::new(sdk), Path::new(output)) {
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
            root_flag,
            root,
            references_flag,
            references,
            output_flag,
            output,
        ] if command == "compile-static"
            && root_flag == "--data-root"
            && references_flag == "--references"
            && output_flag == "--output" =>
        {
            let input = Path::new(input);
            let result = compile_static(input, Path::new(root), Path::new(references))
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
                compile_batch::compile_directory(input, root, output)
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

fn write_compiled_stream(input: &Path, output: &Path, stream: CompiledStream) -> io::Result<()> {
    let stem = input.file_stem().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "input must have a file name")
    })?;
    let files = [
        (".bin", stream.files.bin),
        (".relo", stream.files.relo),
        (".imp", stream.files.imp),
        (".manifest", stream.manifest),
    ];
    fs::create_dir(output)?;
    let mut paths = Vec::new();
    for (extension, bytes) in files {
        let mut name = stem.to_ascii_lowercase();
        name.push(extension);
        paths.push(output.join(name));
        let path = paths.last().unwrap();
        if let Err(error) = fs::write(path, bytes) {
            let error = io::Error::new(error.kind(), format!("{} ({error})", path.display()));
            for path in paths {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_dir(output);
            return Err(error);
        }
    }
    println!("Compiled {}", paths.last().unwrap().display());
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
