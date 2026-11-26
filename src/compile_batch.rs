use crate::compile::asset_workers::worker_count;
use crate::compile::build_log;
use crate::compile::{
    CompiledStream, CustomData, capture_stream_warnings, compile_apt_ui, hash_source_file,
    write_warnings,
};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;

const GENERATED: [&str; 5] = ["manifest", "bin", "relo", "imp", "version"];

pub(crate) enum Contents {
    Bytes(Vec<u8>),
    CustomData(Vec<(String, CustomData)>),
}

pub(crate) type Output = (OsString, Contents);

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
    compile: impl Fn(&Path) -> io::Result<CompiledStream> + Sync,
) -> io::Result<Vec<Output>> {
    let workers = if sources.len() > 1 {
        worker_count()?
    } else {
        1
    };
    let mut names = HashSet::new();
    let jobs: Vec<_> = sources
        .iter()
        .map(|source| (source, source.file_stem().unwrap().to_ascii_lowercase()))
        .take_while(|(_, stem)| names.insert(stem.clone()))
        .collect();
    let mut outputs = Vec::new();
    let mut append =
        |stem: &OsStr, stream| append_versioned_stream(stem, stream, version, &mut outputs);
    for batch in jobs.chunks(workers) {
        if let [(source, stem)] = batch {
            println!("Compiling XML with Rust {}", source.display());
            append(stem, compile(source)?);
            continue;
        }
        thread::scope(|scope| {
            let compile = &compile;
            let workers: Vec<_> = batch
                .iter()
                .map(|(source, _)| {
                    thread::Builder::new().spawn_scoped(scope, move || {
                        build_log::capture(|| capture_stream_warnings(|| compile(source)))
                    })
                })
                .collect();
            let mut result = Ok(());
            for ((source, stem), worker) in batch.iter().zip(workers) {
                let compiled = worker.and_then(|worker| {
                    worker.join().map_err(|_| {
                        io::Error::other(format!(
                            "{}: stream compiler worker panicked",
                            source.display()
                        ))
                    })
                });
                // Join every worker, but report diagnostics only through the first failure.
                if result.is_ok() {
                    println!("Compiling XML with Rust {}", source.display());
                    result = compiled.and_then(|((stream, warnings), log)| {
                        if let Some(log) = log {
                            build_log::write_log(&log);
                        }
                        write_warnings(&warnings);
                        append(stem, stream?);
                        Ok(())
                    });
                }
            }
            result
        })?;
    }
    if jobs.len() < sources.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "duplicate stream output name",
        ));
    }
    Ok(outputs)
}
pub(crate) fn append_versioned_stream(
    stem: &OsStr,
    stream: CompiledStream,
    version: &[u8],
    outputs: &mut Vec<Output>,
) {
    stream_outputs(stem, stem, stream, outputs);
    outputs.push((
        with_suffix(stem, ".version"),
        Contents::Bytes(version.to_vec()),
    ));
}

pub(crate) fn stream_outputs(
    stem: &OsStr,
    custom_data_stem: &OsStr,
    stream: CompiledStream,
    outputs: &mut Vec<Output>,
) {
    for (suffix, bytes) in [
        (".bin", stream.files.bin),
        (".relo", stream.files.relo),
        (".imp", stream.files.imp),
        (".manifest", stream.manifest),
    ] {
        outputs.push((with_suffix(stem, suffix), Contents::Bytes(bytes)));
    }
    outputs.push((
        Path::new(custom_data_stem).join("cdata").into_os_string(),
        Contents::CustomData(stream.custom_data),
    ));
}

pub(crate) fn publish_map_stream(
    output: &Path,
    compile: impl FnOnce() -> io::Result<CompiledStream>,
) -> io::Result<()> {
    publish_prepared(
        output,
        || {
            let mut outputs = Vec::new();
            // Map versions select the linked stream suffix. The compiler's
            // custom-data directory still uses the original map stream name.
            stream_outputs(
                OsStr::new("map.1.12"),
                OsStr::new("map"),
                compile()?,
                &mut outputs,
            );
            outputs.push(("map.version".into(), Contents::Bytes(b".1.12\r\n".to_vec())));
            Ok(outputs)
        },
        |_| false,
    )
}

fn with_suffix(stem: &OsStr, suffix: &str) -> OsString {
    let mut name = stem.to_os_string();
    name.push(suffix);
    name
}

pub fn write_compiled_stream(
    input: &Path,
    output: &Path,
    stream: CompiledStream,
) -> io::Result<()> {
    let stem = input
        .file_stem()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "input must have a file name"))?
        .to_ascii_lowercase();
    fs::create_dir(output)?;
    let mut outputs = Vec::new();
    stream_outputs(&stem, &stem, stream, &mut outputs);
    if let Err(error) = publish(output, outputs, |_| false) {
        let _ = fs::remove_dir(output);
        return Err(error);
    }
    println!(
        "Compiled {}",
        output.join(with_suffix(&stem, ".manifest")).display()
    );
    Ok(())
}

pub(crate) fn publish(
    output: &Path,
    outputs: Vec<Output>,
    replace: impl Fn(&Path) -> bool,
) -> io::Result<()> {
    publish_prepared(output, || Ok(outputs), replace)
}

pub(crate) fn publish_prepared(
    output: &Path,
    prepare: impl FnOnce() -> io::Result<Vec<Output>>,
    replace: impl Fn(&Path) -> bool,
) -> io::Result<()> {
    fs::create_dir_all(output)?;
    let stage = output.join(".rust-streams");
    fs::create_dir(&stage)?;
    let fresh = stage.join("new");
    let old = stage.join("previous");
    let mut backed_up = Vec::new();
    let mut published = Vec::new();
    let mut outputs = match prepare() {
        Ok(outputs) => outputs,
        Err(error) => {
            fs::remove_dir(&stage)?;
            return Err(error);
        }
    };
    outputs.retain(|(name, contents)| {
        !matches!(contents, Contents::CustomData(files) if files.is_empty())
            || output.join(name).exists()
    });
    let result = (|| {
        fs::create_dir(&fresh)?;
        fs::create_dir(&old)?;
        for (name, contents) in &outputs {
            let destination = fresh.join(name);
            match contents {
                Contents::Bytes(bytes) => {
                    fs::create_dir_all(destination.parent().unwrap())?;
                    fs::write(destination, bytes)?;
                }
                Contents::CustomData(files) => {
                    fs::create_dir_all(&destination)?;
                    for (file, data) in files {
                        if Path::new(file).file_name() != Some(file.as_ref()) {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "invalid custom-data file name",
                            ));
                        }
                        let path = destination.join(file);
                        match data {
                            CustomData::Bytes(bytes) => fs::write(path, bytes)?,
                            CustomData::Source(source, hash) => {
                                let staged = fs::File::create(path)?;
                                if hash_source_file(source, staged)? != *hash {
                                    return Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        format!("{} changed during compilation", source.display()),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        let output_names: HashSet<_> = outputs
            .iter()
            .map(|(name, _)| name.to_ascii_lowercase())
            .collect();
        let mut previous = Vec::new();
        for entry in fs::read_dir(output)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && (output_names.contains(&entry.file_name().to_ascii_lowercase())
                    || replace(&entry.path()))
            {
                previous.push(entry.file_name());
            }
        }
        for (name, contents) in &outputs {
            if (matches!(contents, Contents::CustomData(_))
                || Path::new(name).components().count() > 1)
                && output.join(name).exists()
            {
                previous.push(name.clone());
            }
        }
        for name in previous {
            let backup = old.join(&name);
            fs::create_dir_all(backup.parent().unwrap())?;
            fs::rename(output.join(&name), backup)?;
            backed_up.push(name);
        }
        for (name, _) in &outputs {
            let destination = output.join(name);
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::rename(fresh.join(name), destination)?;
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
        let _ = fs::remove_dir_all(dir);
    }
    let _ = fs::remove_dir(stage);
}
