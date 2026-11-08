use crate::compile::{CompiledStream, compile_apt_ui};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const GENERATED: [&str; 5] = ["manifest", "bin", "relo", "imp", "version"];

pub fn compile_apt_ui_directory(input: &Path, root: &Path, output: &Path) -> io::Result<()> {
    let sources = xml_sources(input, |_| true)?;
    if sources.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no AptUI XML streams found",
        ));
    }
    let outputs = compile_streams(&sources, b" \r\n", |source| compile_apt_ui(source, root))?;
    publish(output, outputs, is_generated)?;
    println!("Compiled {} AptUI streams with Rust", sources.len());
    Ok(())
}

pub(crate) fn xml_sources(
    input: &Path,
    selected: impl Fn(&Path) -> bool,
) -> io::Result<Vec<PathBuf>> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(input)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file() && has_extension(&path, "xml") && selected(&path) {
            sources.push(path);
        }
    }
    sources.sort();
    Ok(sources)
}

pub(crate) fn compile_streams(
    sources: &[PathBuf],
    version: &[u8],
    compile: impl Fn(&Path) -> io::Result<CompiledStream>,
) -> io::Result<Vec<(OsString, Vec<u8>)>> {
    let mut names = HashSet::new();
    let mut outputs = Vec::new();
    for source in sources {
        let stem = source.file_stem().unwrap().to_ascii_lowercase();
        if !names.insert(stem.clone()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "duplicate stream output name",
            ));
        }
        println!("Compiling XML with Rust {}", source.display());
        let stream = compile(source)?;
        for (extension, bytes) in [
            ("bin", stream.files.bin),
            ("relo", stream.files.relo),
            ("imp", stream.files.imp),
            ("manifest", stream.manifest),
            ("version", version.to_vec()),
        ] {
            let mut name = stem.clone();
            name.push(".");
            name.push(extension);
            outputs.push((name, bytes));
        }
    }
    Ok(outputs)
}

pub(crate) fn publish(
    output: &Path,
    outputs: Vec<(OsString, Vec<u8>)>,
    replace: impl Fn(&Path) -> bool,
) -> io::Result<()> {
    fs::create_dir_all(output)?;
    let stage = output.join(".rust-streams");
    fs::create_dir(&stage)?;
    let fresh = stage.join("new");
    let old = stage.join("previous");
    let mut backed_up = Vec::new();
    let mut published = Vec::new();
    let result = (|| {
        fs::create_dir(&fresh)?;
        fs::create_dir(&old)?;
        for (name, bytes) in &outputs {
            fs::write(fresh.join(name), bytes)?;
        }
        let mut previous = Vec::new();
        for entry in fs::read_dir(output)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && replace(&entry.path()) {
                previous.push(entry.file_name());
            }
        }
        for name in previous {
            fs::rename(output.join(&name), old.join(&name))?;
            backed_up.push(name);
        }
        for (name, _) in &outputs {
            fs::rename(fresh.join(name), output.join(name))?;
            published.push(name);
        }
        Ok::<(), io::Error>(())
    })();

    if let Err(error) = result {
        if let Err(rollback) = rollback(output, &fresh, &old, &backed_up, &published) {
            return Err(io::Error::other(format!(
                "{error}. Could not restore previous outputs ({rollback}). Recovery files are in {}",
                stage.display()
            )));
        }
        remove_stage(&stage, &fresh, &old);
        return Err(error);
    }
    remove_stage(&stage, &fresh, &old);
    Ok(())
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

pub(crate) fn is_generated(path: &Path) -> bool {
    GENERATED.iter().any(|ext| has_extension(path, ext))
}

fn rollback(
    output: &Path,
    fresh: &Path,
    old: &Path,
    backed_up: &[OsString],
    published: &[&OsString],
) -> io::Result<()> {
    for name in published.iter().rev() {
        fs::rename(output.join(name), fresh.join(name))?;
    }
    for name in backed_up.iter().rev() {
        fs::rename(old.join(name), output.join(name))?;
    }
    Ok(())
}

fn remove_stage(stage: &Path, fresh: &Path, old: &Path) {
    for dir in [fresh, old] {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
            let _ = fs::remove_dir(dir);
        }
    }
    let _ = fs::remove_dir(stage);
}
