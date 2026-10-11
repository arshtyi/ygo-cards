use std::{fmt, sync::Mutex};

use anyhow::Result;

static DIAGNOSTICS: Mutex<Vec<Diagnostic>> = Mutex::new(Vec::new());

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Severity {
    Warning,
    Error,
}

impl Severity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiagnosticField {
    pub(crate) label: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Diagnostic {
    pub(crate) severity: Severity,
    pub(crate) code: &'static str,
    pub(crate) title: String,
    pub(crate) context: Vec<DiagnosticField>,
    pub(crate) reason: Option<String>,
    pub(crate) suggestion: Option<String>,
}

impl Diagnostic {
    pub(crate) fn warning(code: &'static str, title: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, title)
    }

    pub(crate) fn error(code: &'static str, title: impl Into<String>) -> Self {
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

    pub(crate) fn context(mut self, label: impl Into<String>, value: impl fmt::Display) -> Self {
        self.context.push(DiagnosticField {
            label: label.into(),
            value: value.to_string(),
        });
        self
    }

    pub(crate) fn reason(mut self, reason: impl fmt::Display) -> Self {
        self.reason = Some(reason.to_string());
        self
    }

    pub(crate) fn suggestion(mut self, suggestion: impl fmt::Display) -> Self {
        self.suggestion = Some(suggestion.to_string());
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DiagnosticSnapshot {
    pub(crate) records: Vec<Diagnostic>,
    warnings: usize,
    errors: usize,
}

impl DiagnosticSnapshot {
    pub(crate) fn from_records(records: Vec<Diagnostic>) -> Self {
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

    pub(crate) fn warnings(&self) -> usize {
        self.warnings
    }

    pub(crate) fn errors(&self) -> usize {
        self.errors
    }
}

pub(crate) fn record(diagnostic: Diagnostic) {
    if let Ok(mut records) = DIAGNOSTICS.lock() {
        records.push(diagnostic);
    }
}

pub(crate) fn snapshot() -> Result<DiagnosticSnapshot> {
    let records = DIAGNOSTICS
        .lock()
        .map_err(|_| anyhow::anyhow!("diagnostics lock is poisoned"))?;
    Ok(DiagnosticSnapshot::from_records(records.clone()))
}
