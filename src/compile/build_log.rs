use super::CompiledAsset;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::io::{self, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

thread_local! {
    static STREAM_LOG: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn enabled() -> bool {
    std::env::var_os("BAB_DETAILED_BUILDLOG").is_some_and(|value| value == "1")
}

// Batch workers buffer their logs so ModStudio receives one complete stream at a time.
pub(crate) fn capture<T>(run: impl FnOnce() -> T) -> (T, Option<String>) {
    if enabled() {
        STREAM_LOG.set(Some(String::new()));
    }
    (run(), STREAM_LOG.take())
}

pub(crate) fn write_log(log: &str) {
    let _ = io::stdout().lock().write_all(log.as_bytes());
}

pub(super) struct BuildLog {
    start: Instant,
    stream: String,
    stats: BTreeMap<String, AssetStats>,
}

#[derive(Default)]
struct AssetStats {
    count: u32,
    total: Duration,
    longest: Duration,
    slowest: String,
}

impl BuildLog {
    pub(super) fn start(input: &Path, root: &Path) -> Option<Self> {
        if !enabled() {
            return None;
        }
        let stream = input
            .strip_prefix(root)
            .unwrap_or(input)
            .with_extension("")
            .to_string_lossy()
            .to_ascii_lowercase();
        let log = Self {
            start: Instant::now(),
            stream,
            stats: BTreeMap::new(),
        };
        log.line(format_args!("BinaryAssetBuilder started"));
        Some(log)
    }

    pub(super) fn line(&self, message: fmt::Arguments<'_>) {
        let mut line = String::new();
        let _ = writeln!(line, "[{}] {message}", clock(self.start.elapsed()));
        STREAM_LOG.with_borrow_mut(|buffer| {
            if let Some(buffer) = buffer {
                buffer.push_str(&line);
            } else {
                write_log(&line);
            }
        });
    }

    pub(super) fn phase(&self, label: &str) {
        self.line(format_args!("{label}: {}", self.stream));
    }

    pub(super) fn stream_event(&self, label: &str) {
        self.line(format_args!("{} {label}", self.stream));
    }

    pub(super) fn asset(&mut self, path: &Path, asset: &CompiledAsset, elapsed: Duration) {
        let Some((kind, name)) = asset.name.split_once(':') else {
            return;
        };
        let file = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        self.line(format_args!(
            "{file}: Compiling {} [{:08x}:{:08x}]",
            asset.name, asset.id.type_id, asset.id.instance_id
        ));
        let stats = self.stats.entry(kind.to_owned()).or_default();
        stats.count += 1;
        stats.total += elapsed;
        if stats.count == 1 || elapsed > stats.longest {
            stats.longest = elapsed;
            stats.slowest = name.to_owned();
        }
    }

    pub(super) fn complete(&self) {
        self.stream_event("Stream complete");
        self.line(format_args!("BinaryAssetBuilder complete"));
        let mut total = Duration::ZERO;
        for (kind, stats) in &self.stats {
            self.line(format_args!("{kind}:"));
            self.line(format_args!("      Instances Processed: {}", stats.count));
            self.line(format_args!(
                "      Total Processing Time: {}",
                clock(stats.total)
            ));
            self.line(format_args!(
                "      Longest Processing Time: {} ({})",
                clock(stats.longest),
                stats.slowest
            ));
            self.line(format_args!(
                "      Average Processing Time: {:.7} seconds",
                (stats.total / stats.count).as_secs_f64()
            ));
            total += stats.total;
        }
        self.line(format_args!(
            "Total Processing Time for All Types: {}",
            clock(total)
        ));
    }
}

fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    format!(
        "{:02}:{:02}:{:02}.{:07}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60,
        elapsed.subsec_nanos() / 100
    )
}
