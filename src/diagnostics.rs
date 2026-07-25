use std::{
    fmt,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};

pub const BUILD_LOG_PATH: &str = "output/build.log";

static BUILD_LOG: OnceLock<Mutex<BuildLog>> = OnceLock::new();

struct BuildLog {
    path: PathBuf,
    writer: BufWriter<File>,
    write_error: Option<String>,
    records: usize,
}

#[derive(Clone, Copy)]
enum Level {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Error => "ERROR",
        })
    }
}

impl BuildLog {
    fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create build log directory {}", parent.display())
            })?;
        }

        let file = File::create(path)
            .with_context(|| format!("failed to create build log {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        writeln!(
            writer,
            "ygo-cards {} build diagnostics",
            env!("CARGO_PKG_VERSION")
        )
        .with_context(|| format!("failed to initialize build log {}", path.display()))?;

        Ok(Self {
            path: path.to_path_buf(),
            writer,
            write_error: None,
            records: 0,
        })
    }

    fn write(&mut self, level: Level, message: fmt::Arguments<'_>) {
        if self.write_error.is_some() {
            return;
        }

        match write_record(&mut self.writer, level, message) {
            Ok(()) => self.records += 1,
            Err(error) => self.write_error = Some(error.to_string()),
        }
    }

    fn finish(&mut self) -> Result<()> {
        if self.records == 0 {
            self.write(
                Level::Info,
                format_args!("build completed without warnings or errors"),
            );
        }

        if let Some(error) = &self.write_error {
            bail!("failed to write build log {}: {error}", self.path.display());
        }

        self.writer
            .flush()
            .with_context(|| format!("failed to flush build log {}", self.path.display()))
    }
}

fn write_record(
    writer: &mut impl Write,
    level: Level,
    message: fmt::Arguments<'_>,
) -> io::Result<()> {
    writeln!(writer, "[{level}] {message}")
}

pub fn init() -> Result<PathBuf> {
    let path = PathBuf::from(BUILD_LOG_PATH);
    let log = BuildLog::create(&path)?;
    BUILD_LOG
        .set(Mutex::new(log))
        .map_err(|_| anyhow::anyhow!("build log has already been initialized"))?;
    Ok(path)
}

pub fn warning(message: fmt::Arguments<'_>) {
    write(Level::Warning, message);
}

pub fn error(message: fmt::Arguments<'_>) {
    write(Level::Error, message);
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        error(format_args!("unexpected panic: {panic_info}"));
        let _ = finish();
    }));
}

fn write(level: Level, message: fmt::Arguments<'_>) {
    let Some(log) = BUILD_LOG.get() else {
        return;
    };
    let Ok(mut log) = log.lock() else {
        return;
    };
    log.write(level, message);
}

pub fn finish() -> Result<()> {
    let Some(log) = BUILD_LOG.get() else {
        return Ok(());
    };
    let mut log = log
        .lock()
        .map_err(|_| anyhow::anyhow!("build log lock is poisoned"))?;
    log.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_diagnostic_records() {
        let mut output = Vec::new();

        write_record(
            &mut output,
            Level::Warning,
            format_args!("card id={} failed", 42),
        )
        .unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[WARN] card id=42 failed\n"
        );
    }

    #[test]
    fn build_log_path_is_scoped_to_output() {
        assert!(Path::new(BUILD_LOG_PATH).starts_with("output"));
    }
}
