#![forbid(unsafe_code)]

use binary_asset_builder::compile::compile_apt_ui;
use binary_asset_builder::linked_stream::check_linked_files;
use binary_asset_builder::manifest::{ByteOrder, Manifest};
use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
Usage
  binary-asset-builder compile <file.xml> --data-root <directory> --output <new-directory>
  binary-asset-builder inspect <file.manifest>
  binary-asset-builder check-linked <file.manifest>";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--help" || flag == "-h" || flag == "/?" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [command, input, root_flag, root, output_flag, output]
            if command == "compile" && root_flag == "--data-root" && output_flag == "--output" =>
        {
            let input = Path::new(input);
            match compile(input, Path::new(root), Path::new(output)) {
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
                "Use the compile command for supported AptUI XML streams. See --help for {}.",
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

fn compile(input: &Path, root: &Path, output: &Path) -> io::Result<()> {
    let stem = input.file_stem().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "input must have a file name")
    })?;
    let stream = compile_apt_ui(input, root)?;
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
