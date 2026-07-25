use std::fmt;

use rusqlite::Error;

use crate::diagnostics;

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
        diagnostics::warning(format_args!(
            "skip {} card: id={} name={:?} reason={reason}",
            self.environment, self.id, self.name
        ));
    }
}

pub(crate) fn card(
    environment: &'static str,
    id: i64,
    name: Option<&str>,
    reason: fmt::Arguments<'_>,
) {
    match name {
        Some(name) => diagnostics::warning(format_args!(
            "skip {environment} card: id={id} name={name:?} reason={reason}"
        )),
        None => diagnostics::warning(format_args!(
            "skip {environment} card: id={id} reason={reason}"
        )),
    }
}

pub(crate) fn database_row(
    environment: &'static str,
    row_number: usize,
    error: &Error,
) {
    diagnostics::warning(format_args!(
        "skip {environment} database row: row_number={row_number} reason=failed to decode SQLite row: {error}"
    ));
}
