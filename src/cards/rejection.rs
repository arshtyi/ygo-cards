use std::fmt;

use rusqlite::Error;

use crate::diagnostics::{self, Diagnostic};

pub(crate) struct Card<'a> {
    environment: &'static str,
    id: i64,
    name: &'a str,
}

impl<'a> Card<'a> {
    pub(crate) fn new(environment: &'static str, id: i64, name: &'a str) -> Self {
        Self {
            environment,
            id,
            name,
        }
    }

    pub(crate) fn warning(&self, reason: fmt::Arguments<'_>) {
        diagnostics::record(
            Diagnostic::warning("card.skipped", "Card was skipped")
                .context("Environment", self.environment)
                .context("Card ID", self.id)
                .context("Name", self.name)
                .reason(reason),
        );
    }
}

pub(crate) fn card(
    environment: &'static str,
    id: i64,
    name: Option<&str>,
    reason: fmt::Arguments<'_>,
) {
    let mut diagnostic = Diagnostic::warning("card.skipped", "Card was skipped")
        .context("Environment", environment)
        .context("Card ID", id)
        .reason(reason);
    if let Some(name) = name {
        diagnostic = diagnostic.context("Name", name);
    }
    diagnostics::record(diagnostic);
}

pub(crate) fn database_row(environment: &'static str, row_number: usize, error: &Error) {
    diagnostics::record(
        Diagnostic::warning("database.row-skipped", "Database row was skipped")
            .context("Environment", environment)
            .context("Row", row_number)
            .reason(format!("failed to decode SQLite row: {error}")),
    );
}
