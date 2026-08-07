use std::{
    any::Any,
    backtrace::{Backtrace, BacktraceStatus},
    fmt,
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, bail};

pub const BUILD_LOG_PATH: &str = "output/build.log";

static BUILD_LOG: OnceLock<Mutex<BuildLog>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticField {
    label: String,
    value: String,
}

impl DiagnosticField {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    severity: Severity,
    code: &'static str,
    title: String,
    context: Vec<DiagnosticField>,
    reason: Option<String>,
    suggestion: Option<String>,
}

impl Diagnostic {
    pub fn warning(code: &'static str, title: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, title)
    }

    pub fn error(code: &'static str, title: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, title)
    }

    fn new(severity: Severity, code: &'static str, title: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            title: title.into(),
            context: Vec::new(),
            reason: None,
            suggestion: None,
        }
    }

    pub fn context(mut self, label: impl Into<String>, value: impl fmt::Display) -> Self {
        self.context.push(DiagnosticField {
            label: label.into(),
            value: value.to_string(),
        });
        self
    }

    pub fn reason(mut self, reason: impl fmt::Display) -> Self {
        self.reason = Some(reason.to_string());
        self
    }

    pub fn suggestion(mut self, suggestion: impl fmt::Display) -> Self {
        self.suggestion = Some(suggestion.to_string());
        self
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn code(&self) -> &str {
        self.code
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn fields(&self) -> &[DiagnosticField] {
        &self.context
    }

    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn suggestion_text(&self) -> Option<&str> {
        self.suggestion.as_deref()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiagnosticSnapshot {
    records: Vec<Diagnostic>,
    warnings: usize,
    errors: usize,
}

impl DiagnosticSnapshot {
    pub fn from_records(records: Vec<Diagnostic>) -> Self {
        let warnings = records
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
            .count();
        let errors = records.len() - warnings;
        Self {
            records,
            warnings,
            errors,
        }
    }

    pub fn records(&self) -> &[Diagnostic] {
        &self.records
    }

    pub fn warnings(&self) -> usize {
        self.warnings
    }

    pub fn errors(&self) -> usize {
        self.errors
    }

    pub fn is_clean(&self) -> bool {
        self.records.is_empty()
    }
}

struct BuildLog {
    path: PathBuf,
    writer: BufWriter<File>,
    write_error: Option<String>,
    records: Vec<Diagnostic>,
    warnings: usize,
    errors: usize,
    finished: bool,
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
        write_header(&mut writer)
            .with_context(|| format!("failed to initialize build log {}", path.display()))?;

        Ok(Self {
            path: path.to_path_buf(),
            writer,
            write_error: None,
            records: Vec::new(),
            warnings: 0,
            errors: 0,
            finished: false,
        })
    }

    fn record(&mut self, diagnostic: Diagnostic) {
        if self.finished {
            return;
        }

        match diagnostic.severity {
            Severity::Warning => self.warnings += 1,
            Severity::Error => self.errors += 1,
        }
        self.records.push(diagnostic);

        if self.write_error.is_some() {
            return;
        }

        let number = self.records.len();
        if let Err(error) = write_record(&mut self.writer, number, &self.records[number - 1]) {
            self.write_error = Some(error.to_string());
        }
    }

    fn snapshot(&self) -> DiagnosticSnapshot {
        DiagnosticSnapshot {
            records: self.records.clone(),
            warnings: self.warnings,
            errors: self.errors,
        }
    }

    fn finish(&mut self) -> Result<()> {
        if self.finished {
            return self.check_write_error();
        }
        self.finished = true;

        if self.write_error.is_none() {
            let result = if self.records.is_empty() {
                writeln!(self.writer, "No warnings or errors were recorded.\n")
                    .and_then(|_| write_summary(&mut self.writer, self.warnings, self.errors))
            } else {
                write_summary(&mut self.writer, self.warnings, self.errors)
            };
            if let Err(error) = result {
                self.write_error = Some(error.to_string());
            }
        }

        self.check_write_error()?;
        if let Err(error) = self.writer.flush() {
            self.write_error = Some(format!("failed to flush buffered data: {error}"));
        }
        self.check_write_error()
    }

    fn check_write_error(&self) -> Result<()> {
        if let Some(error) = &self.write_error {
            bail!("failed to write build log {}: {error}", self.path.display());
        }
        Ok(())
    }
}

fn write_header(writer: &mut impl Write) -> io::Result<()> {
    writeln!(writer, "ygo-cards build diagnostics")?;
    writeln!(writer, "===========================")?;
    writeln!(writer, "Version: {}\n", env!("CARGO_PKG_VERSION"))
}

fn write_record(writer: &mut impl Write, number: usize, diagnostic: &Diagnostic) -> io::Result<()> {
    writeln!(
        writer,
        "[{} {number:03}] {}",
        diagnostic.severity.label(),
        diagnostic.title
    )?;

    let label_width = diagnostic
        .context
        .iter()
        .map(|field| field.label.chars().count())
        .chain([
            "Code".len(),
            usize::from(diagnostic.reason.is_some()) * "Reason".len(),
            usize::from(diagnostic.suggestion.is_some()) * "Suggestion".len(),
        ])
        .max()
        .unwrap_or("Code".len());

    write_field(writer, "Code", diagnostic.code, label_width)?;
    for field in &diagnostic.context {
        write_field(writer, &field.label, &field.value, label_width)?;
    }
    if let Some(reason) = &diagnostic.reason {
        write_field(writer, "Reason", reason, label_width)?;
    }
    if let Some(suggestion) = &diagnostic.suggestion {
        write_field(writer, "Suggestion", suggestion, label_width)?;
    }
    writeln!(writer)
}

fn write_field(
    writer: &mut impl Write,
    label: &str,
    value: &str,
    label_width: usize,
) -> io::Result<()> {
    const MAX_LINE_WIDTH: usize = 100;

    let value_width = MAX_LINE_WIDTH.saturating_sub(label_width + 5).max(20);
    let wrapped = value
        .lines()
        .flat_map(|line| wrap_line(line, value_width))
        .collect::<Vec<_>>();
    let mut lines = wrapped.iter();
    let first = lines.next().map(String::as_str).unwrap_or_default();
    writeln!(writer, "  {label:<label_width$} : {first}")?;
    let continuation_indent = " ".repeat(label_width + 5);
    for line in lines {
        writeln!(writer, "{continuation_indent}{line}")?;
    }
    Ok(())
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let mut remainder = line.trim_end();
    let mut lines = Vec::new();
    while remainder.chars().count() > width {
        let split = remainder
            .char_indices()
            .enumerate()
            .take_while(|(position, _)| *position <= width)
            .filter_map(|(_, (index, character))| character.is_whitespace().then_some(index))
            .last();
        let Some(split) = split else {
            break;
        };
        lines.push(remainder[..split].trim_end().to_string());
        remainder = remainder[split..].trim_start();
    }
    lines.push(remainder.to_string());
    lines
}

fn write_summary(writer: &mut impl Write, warnings: usize, errors: usize) -> io::Result<()> {
    writeln!(writer, "Summary")?;
    writeln!(writer, "-------")?;
    writeln!(writer, "Warnings : {warnings}")?;
    writeln!(writer, "Errors   : {errors}")
}

pub fn init() -> Result<PathBuf> {
    if BUILD_LOG.get().is_some() {
        bail!("build log has already been initialized");
    }

    let path = PathBuf::from(BUILD_LOG_PATH);
    let log = BuildLog::create(&path)?;
    BUILD_LOG
        .set(Mutex::new(log))
        .map_err(|_| anyhow::anyhow!("build log has already been initialized"))?;
    Ok(path)
}

pub fn record(diagnostic: Diagnostic) {
    let Some(log) = BUILD_LOG.get() else {
        return;
    };
    let Ok(mut log) = log.lock() else {
        return;
    };
    log.record(diagnostic);
}

pub fn snapshot() -> Result<DiagnosticSnapshot> {
    let Some(log) = BUILD_LOG.get() else {
        return Ok(DiagnosticSnapshot::default());
    };
    let log = log
        .lock()
        .map_err(|_| anyhow::anyhow!("build log lock is poisoned"))?;
    Ok(log.snapshot())
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = panic_message(panic_info.payload());
        let backtrace = Backtrace::capture();
        let mut diagnostic =
            Diagnostic::error("runtime.panic", "Unexpected internal error").reason(&message);
        if let Some(location) = panic_info.location() {
            diagnostic = diagnostic.context(
                "Location",
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                ),
            );
        }
        if backtrace.status() == BacktraceStatus::Captured {
            diagnostic = diagnostic
                .context("Backtrace", &backtrace)
                .suggestion("Include the build log when reporting this internal error");
        } else {
            diagnostic = diagnostic.suggestion(
                "Re-run with RUST_BACKTRACE=1 and include the build log when reporting it",
            );
        }
        record(diagnostic);
        let finish_error = finish().err();

        eprintln!("\nUnexpected internal error");
        eprintln!("  {message}");
        if let Some(location) = panic_info.location() {
            eprintln!(
                "  at {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }
        if backtrace.status() == BacktraceStatus::Captured {
            eprintln!("\n{backtrace}");
        }
        eprintln!("  Diagnostics: {BUILD_LOG_PATH}");
        if let Some(error) = finish_error {
            eprintln!("  Could not finish diagnostics: {error:#}");
        }
    }));
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| String::from("panic payload was not a string"))
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
    fn formats_structured_diagnostic_records() {
        let mut output = Vec::new();
        let diagnostic = Diagnostic::warning("card.skipped", "Card was skipped")
            .context("Environment", "OT")
            .context("Card ID", 42)
            .reason("name is empty\nvalue cannot be normalized")
            .suggestion("Inspect the upstream card text");

        write_record(&mut output, 7, &diagnostic).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[WARNING 007] Card was skipped\n\
             \x20 Code        : card.skipped\n\
             \x20 Environment : OT\n\
             \x20 Card ID     : 42\n\
             \x20 Reason      : name is empty\n\
             \x20               value cannot be normalized\n\
             \x20 Suggestion  : Inspect the upstream card text\n\n"
        );
    }

    #[test]
    fn formats_clean_summary() {
        let mut output = Vec::new();
        write_summary(&mut output, 0, 0).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "Summary\n-------\nWarnings : 0\nErrors   : 0\n"
        );
    }

    #[test]
    fn wraps_long_diagnostic_values_at_word_boundaries() {
        let mut output = Vec::new();
        let diagnostic = Diagnostic::warning("network.retry", "Request will be retried").reason(
            "download failed because the remote endpoint closed the connection before the full response body was received; the request can be retried safely",
        );

        write_record(&mut output, 1, &diagnostic).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("download failed because"));
        assert!(output.contains("retried safely"));
        assert_eq!(
            output
                .lines()
                .filter(|line| line.contains("received"))
                .count(),
            1
        );
        assert!(output.lines().all(|line| line.chars().count() <= 100));
    }

    #[test]
    fn build_log_path_is_scoped_to_output() {
        assert!(Path::new(BUILD_LOG_PATH).starts_with("output"));
    }
}
