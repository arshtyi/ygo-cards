use std::{fmt::Write, fs, path::Path};

use anyhow::{Context, Result};

use crate::{
    cards::DatasetReport,
    diagnostics::{Diagnostic, DiagnosticSnapshot, Severity},
    formatting::format_count,
    latest::LatestReleaseComparison,
};

pub(crate) const SUMMARY_REPORT: &str = "output/report.md";

pub(crate) enum ReportScope<'a> {
    Full(&'a LatestReleaseComparison),
    Limited(usize),
}

pub(crate) fn write_report(
    reports: &[&DatasetReport],
    scope: ReportScope<'_>,
    diagnostics: &DiagnosticSnapshot,
    path: &Path,
) -> Result<()> {
    fs::write(path, build_report(reports, scope, diagnostics))
        .with_context(|| format!("failed to write {}", path.display()))
}

pub(crate) fn write_failure_report(diagnostics: &DiagnosticSnapshot, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }
    fs::write(path, build_failure_report(diagnostics))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn build_failure_report(diagnostics: &DiagnosticSnapshot) -> String {
    let mut report = String::from("# Card Data Report\n\nBuild failed; outputs are incomplete.\n");
    append_diagnostics(&mut report, diagnostics);
    report
}

fn build_report(
    reports: &[&DatasetReport],
    scope: ReportScope<'_>,
    diagnostics: &DiagnosticSnapshot,
) -> String {
    let mut report = String::from("# Card Data Report\n\n");
    match scope {
        ReportScope::Full(comparison) => append_comparison(&mut report, reports, comparison),
        ReportScope::Limited(limit) => {
            let _ = writeln!(
                report,
                "Limited test run: at most {} input cards per dataset, in ID order. Outputs and card diagnostics cover only this sample; release comparison and diffs are skipped.\n",
                format_count(limit)
            );
            report.push_str(
                "| Dataset | Processed | Written | Skipped |\n| --- | ---: | ---: | ---: |\n",
            );
            for dataset in reports {
                let _ = writeln!(
                    report,
                    "| {} | {} | {} | {} |",
                    dataset.environment,
                    format_count(dataset.cards_written + dataset.cards_skipped),
                    format_count(dataset.cards_written),
                    format_count(dataset.cards_skipped),
                );
            }
        }
    }

    report.push_str("\n## Image Validation\n\n");
    let images_checked = reports.iter().any(|dataset| dataset.image_summary.enabled);
    if images_checked {
        report.push_str("| Dataset | Checked | Primary found | Alias found | Unresolved | Skipped for images | Transport failures (URLs) | Failure policy |\n");
        report.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
        for dataset in reports {
            let images = dataset.image_summary;
            if images.enabled {
                let policy = if images.skip_failures {
                    "skip card"
                } else {
                    "image = 0"
                };
                let _ = writeln!(
                    report,
                    "| {} | {} | {} | {} | {} | {} | {} | {} |",
                    dataset.environment,
                    format_count(images.cards_checked),
                    format_count(images.primary_found),
                    format_count(images.alias_found),
                    format_count(images.missing),
                    format_count(images.cards_skipped),
                    format_count(images.network_errors),
                    policy
                );
            }
        }
        report.push_str("\nTransport failures exclude HTTP responses such as 404. Failed primary candidates can still resolve through an alias.\n");
    } else {
        report.push_str("Not checked. Card IDs were used as image IDs.\n");
    }
    append_diagnostics(&mut report, diagnostics);
    report
}

fn append_comparison(
    report: &mut String,
    reports: &[&DatasetReport],
    comparison: &LatestReleaseComparison,
) {
    if let Some(release) = &comparison.release {
        let _ = writeln!(
            report,
            "Previous release: [{}]({}).\n",
            escape_cell(&release.tag),
            release.url.replace('(', "%28").replace(')', "%29")
        );
    } else {
        report.push_str("First release; no comparison baseline.\n\n");
    }
    report.push_str("| Dataset | Before | After | Added | Modified | Removed | Skipped |\n");
    report.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: |\n");
    for dataset in reports {
        let changes = comparison
            .datasets
            .iter()
            .find(|changes| changes.environment == dataset.environment);
        let (before, added, modified, removed) = match changes
            .and_then(|changes| changes.previous_cards.map(|before| (changes, before)))
        {
            Some((changes, before)) => (
                format_count(before),
                format_count(changes.added_cards.len()),
                format_count(changes.modified_cards),
                format_count(changes.removed_cards),
            ),
            _ => ("—".into(), "—".into(), "—".into(), "—".into()),
        };
        let _ = writeln!(
            report,
            "| {} | {} | {} | {} | {} | {} | {} |",
            dataset.environment,
            before,
            format_count(dataset.cards_written),
            added,
            modified,
            removed,
            format_count(dataset.cards_skipped)
        );
    }

    let mut additions = comparison
        .datasets
        .iter()
        .filter(|changes| !changes.added_cards.is_empty())
        .peekable();
    if comparison.release.is_some() && additions.peek().is_some() {
        report.push_str("\n## Added Cards\n");
        for changes in additions {
            let _ = writeln!(report, "\n### {}\n", changes.environment);
            report.push_str("| ID | Name | Type |\n| ---: | --- | --- |\n");
            for card in &changes.added_cards {
                let _ = writeln!(
                    report,
                    "| {} | {} | {} |",
                    card.id,
                    escape_cell(&card.name),
                    escape_cell(&card.card_type.join(" / "))
                );
            }
        }
    }
}

fn is_image_warning(diagnostic: &Diagnostic) -> bool {
    diagnostic.severity == Severity::Warning
        && matches!(
            diagnostic.code,
            "image.unresolved" | "image.candidate-failed" | "image.request-retry"
        )
}

fn append_image_diagnostics(report: &mut String, diagnostics: &[&Diagnostic]) {
    if diagnostics.is_empty() {
        return;
    }
    let _ = writeln!(
        report,
        "\n<details>\n<summary>Image validation (warning records: {})</summary>\n\nRecords count failed candidate URLs, unresolved cards, and request retries separately; they are not a count of affected cards.",
        format_count(diagnostics.len())
    );
    for (code, title, columns) in [
        (
            "image.unresolved",
            "Unresolved cards",
            &["Environment", "Card ID", "Alias", "Name", "Action"][..],
        ),
        (
            "image.candidate-failed",
            "Failed image candidates",
            &["Image ID", "Kind", "URL", "Reason"][..],
        ),
        (
            "image.request-retry",
            "Request retries",
            &["Method", "URL", "Attempt", "Retry in", "Reason"][..],
        ),
    ] {
        let mut records = diagnostics.iter().filter(|d| d.code == code).peekable();
        if records.peek().is_none() {
            continue;
        }
        let _ = writeln!(
            report,
            "\n### {title}\n\n| {} |\n| {} |",
            columns.join(" | "),
            vec!["---"; columns.len()].join(" | ")
        );
        for diagnostic in records {
            let cells = columns
                .iter()
                .map(|label| {
                    let value = if *label == "Reason" {
                        diagnostic.reason.as_deref()
                    } else {
                        diagnostic
                            .context
                            .iter()
                            .find(|field| field.label == *label)
                            .map(|field| field.value.as_str())
                    };
                    escape_cell(value.unwrap_or("—"))
                })
                .collect::<Vec<_>>();
            let _ = writeln!(report, "| {} |", cells.join(" | "));
        }
    }
    report.push_str("\n</details>\n");
}

fn append_diagnostics(report: &mut String, diagnostics: &DiagnosticSnapshot) {
    report.push_str("\n## Diagnostics\n");
    if diagnostics.records.is_empty() {
        report.push_str("\nNo warnings or errors were recorded in this run.\n");
        return;
    }

    // Every record belongs to exactly one section. Only known image warnings use
    // compact tables; errors and other codes retain their complete details.
    let (images, other): (Vec<_>, Vec<_>) = diagnostics
        .records
        .iter()
        .partition(|d| is_image_warning(d));
    let warnings = other
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    let errors = other.len() - warnings;
    if other.is_empty() {
        report.push_str("\nOther diagnostics: no warnings or errors were recorded in this run.\n");
    } else {
        let _ = writeln!(
            report,
            "\n<details>\n<summary>Other diagnostics (warnings: {}, errors: {})</summary>",
            format_count(warnings),
            format_count(errors)
        );
        for diagnostic in other {
            append_diagnostic(report, diagnostic);
        }
        report.push_str("\n</details>\n");
    }
    append_image_diagnostics(report, &images);
}

fn append_diagnostic(report: &mut String, diagnostic: &Diagnostic) {
    let _ = writeln!(
        report,
        "\n### {}: {}\n\n| Field | Value |\n| --- | --- |\n| Code | {} |",
        diagnostic.severity.label(),
        escape_cell(&diagnostic.title),
        diagnostic.code
    );
    for field in &diagnostic.context {
        let _ = writeln!(
            report,
            "| {} | {} |",
            escape_cell(&field.label),
            escape_cell(&field.value)
        );
    }
    if let Some(reason) = &diagnostic.reason {
        let _ = writeln!(report, "| Reason | {} |", escape_cell(reason));
    }
    if let Some(suggestion) = &diagnostic.suggestion {
        let _ = writeln!(report, "| Suggestion | {} |", escape_cell(suggestion));
    }
}

fn escape_cell(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
        .replace("\r\n", "\n")
        .replace(['\r', '\n'], "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cards::ImageCheckSummary,
        environment::Environment,
        latest::{AddedCard, LatestComparisonReport, ReleaseReference},
    };

    fn dataset() -> DatasetReport {
        DatasetReport {
            environment: Environment::Ot,
            path: "output/ot.json".into(),
            cards_written: 10,
            cards_skipped: 1,
            image_summary: ImageCheckSummary::default(),
        }
    }

    fn comparison() -> LatestReleaseComparison {
        LatestReleaseComparison {
            release: Some(ReleaseReference {
                tag: "0.0.1".into(),
                url: "https://github.com/example/cards/releases/tag/0.0.1".into(),
            }),
            datasets: vec![LatestComparisonReport {
                environment: Environment::Ot,
                previous_cards: Some(9),
                added_cards: vec![AddedCard {
                    id: 42,
                    name: "新卡 | <测试>".into(),
                    card_type: vec!["怪兽".into(), "效果".into()],
                }],
                modified_cards: 3,
                removed_cards: 0,
                diff: "--- a/ot.json\n+++ b/ot.json\n@@ -1 +1 @@\n-old\n+new\n".into(),
            }],
        }
    }

    #[test]
    fn lists_added_cards_without_duplicate_counts_or_diff_contents() {
        let dataset = dataset();
        let mut comparison = comparison();
        let report = build_report(
            &[&dataset],
            ReportScope::Full(&comparison),
            &DiagnosticSnapshot::default(),
        );
        assert!(report.contains(
            "Previous release: [0.0.1](https://github.com/example/cards/releases/tag/0.0.1)"
        ));
        assert_eq!(report.matches("| OT |").count(), 1);
        assert!(report.contains("| OT | 9 | 10 | 1 | 3 | 0 | 1 |"));
        assert!(report.contains("| 42 | 新卡 \\| &lt;测试&gt; | 怪兽 / 效果 |"));
        assert!(report.contains("Not checked. Card IDs were used as image IDs."));
        comparison.datasets[0].diff = "x".repeat(1_000_000);
        assert_eq!(
            report,
            build_report(
                &[&dataset],
                ReportScope::Full(&comparison),
                &DiagnosticSnapshot::default()
            )
        );
        assert!(!report.contains("```diff"));
    }

    #[test]
    fn first_release_does_not_claim_every_card_is_new() {
        let dataset = dataset();
        let mut comparison = comparison();
        comparison.release = None;
        comparison.datasets[0].previous_cards = None;
        comparison.datasets[0].added_cards.clear();
        let report = build_report(
            &[&dataset],
            ReportScope::Full(&comparison),
            &DiagnosticSnapshot::default(),
        );
        assert!(report.contains("First release; no comparison baseline."));
        assert!(report.contains("| OT | — | 10 | — | — | — | 1 |"));
        assert!(!report.contains("## Added Cards"));
    }

    #[test]
    fn lists_ot_and_rd_additions_separately() {
        let mut rd = dataset();
        rd.environment = Environment::Rd;
        let mut comparison = comparison();
        comparison.datasets.push(LatestComparisonReport {
            environment: Environment::Rd,
            previous_cards: Some(10),
            added_cards: vec![AddedCard {
                id: 43,
                name: "RD card".into(),
                card_type: vec![],
            }],
            modified_cards: 0,
            removed_cards: 1,
            diff: String::new(),
        });
        let report = build_report(
            &[&dataset(), &rd],
            ReportScope::Full(&comparison),
            &DiagnosticSnapshot::default(),
        );
        let ot = report
            .split("### OT")
            .nth(1)
            .unwrap()
            .split("### RD")
            .next()
            .unwrap();
        let rd_section = report.split("### RD").nth(1).unwrap();
        assert!(ot.contains("| 42 |"));
        assert!(!ot.contains("| 43 |"));
        assert!(rd_section.contains("| 43 | RD card |"));
        comparison.datasets[1].added_cards.clear();
        let report = build_report(
            &[&dataset(), &rd],
            ReportScope::Full(&comparison),
            &DiagnosticSnapshot::default(),
        );
        assert!(report.contains("### OT"));
        assert!(!report.contains("### RD"));
    }

    #[test]
    fn unchanged_data_keeps_zero_counts_without_empty_detail_sections() {
        let dataset = dataset();
        let mut comparison = comparison();
        let changes = &mut comparison.datasets[0];
        changes.previous_cards = Some(dataset.cards_written);
        changes.added_cards.clear();
        changes.modified_cards = 0;
        changes.removed_cards = 0;
        changes.diff.clear();
        let report = build_report(
            &[&dataset],
            ReportScope::Full(&comparison),
            &DiagnosticSnapshot::default(),
        );
        assert!(report.contains("| OT | 10 | 10 | 0 | 0 | 0 | 1 |"));
        assert!(report.contains("No warnings or errors were recorded in this run."));
        assert!(!report.contains("## Added Cards"));
        assert!(!report.contains("<details>"));
    }

    #[test]
    fn limited_run_reports_only_sample_counts_and_diagnostics() {
        let diagnostics = DiagnosticSnapshot::from_records(vec![
            Diagnostic::warning("card.skipped", "Card was skipped")
                .context("Card ID", 42)
                .reason("invalid card"),
        ]);
        let report = build_report(&[&dataset()], ReportScope::Limited(20), &diagnostics);
        assert!(report.contains("Limited test run: at most 20 input cards per dataset"));
        assert!(report.contains("| Dataset | Processed | Written | Skipped |"));
        assert!(report.contains("| OT | 11 | 10 | 1 |"));
        assert!(report.contains("invalid card"));
        for text in [
            "Previous release:",
            "First release",
            "Before",
            "Added",
            "Modified",
            "Removed",
        ] {
            assert!(
                !report.contains(text),
                "unexpected full-data information: {text}"
            );
        }
    }

    #[test]
    fn many_image_failures_use_one_table_without_per_record_details() {
        let diagnostics = DiagnosticSnapshot::from_records(
            (1..=100)
                .map(|id| {
                    Diagnostic::warning("image.candidate-failed", "Image candidate check failed")
                        .context("Image ID", id)
                        .context("Kind", "http-or-content")
                        .context("URL", format!("https://example.test/{id}.jpg"))
                        .reason("HTTP 404 Not Found")
                })
                .collect(),
        );
        let report = build_failure_report(&diagnostics);
        assert!(report.contains("<summary>Image validation (warning records: 100)</summary>"));
        assert_eq!(
            report.matches("| Image ID | Kind | URL | Reason |").count(),
            1
        );
        assert_eq!(report.matches("HTTP 404 Not Found").count(), 100);
        assert!(!report.contains("| Field | Value |"));
        assert!(!report.contains("### WARNING:"));
        assert_eq!(report.matches("<details>").count(), 1);
        assert!(!report.contains("<details open"));
    }

    #[test]
    fn preserves_image_failures_and_error_details_once() {
        let mut dataset = dataset();
        dataset.image_summary = ImageCheckSummary {
            enabled: true,
            cards_checked: 11,
            primary_found: 9,
            alias_found: 1,
            missing: 1,
            cards_skipped: 1,
            network_errors: 1,
            skip_failures: true,
        };
        let diagnostics = DiagnosticSnapshot::from_records(vec![
            Diagnostic::warning("image.candidate-failed", "Image candidate check failed")
                .context("Image ID", 42)
                .context("URL", "https://example.test/42.jpg")
                .reason("HTTP 404\nNot Found"),
            Diagnostic::warning("image.unresolved", "Card image could not be resolved")
                .context("Environment", "OT")
                .context("Card ID", 42)
                .context("Alias", 43)
                .context("Name", "Missing | <Image>")
                .context("Action", "card skipped"),
            Diagnostic::warning("image.request-retry", "Image request will be retried")
                .context("Method", "HEAD")
                .context("URL", "https://example.test/43.jpg")
                .context("Attempt", "1 of 3")
                .context("Retry in", "1 seconds")
                .reason("HTTP 503 Service Unavailable"),
            Diagnostic::error("card.invalid", "Card normalization failed")
                .context("Card ID", 99)
                .reason("invalid type\r\nmissing race")
                .suggestion("Check the source"),
        ]);
        let report = build_report(&[&dataset], ReportScope::Full(&comparison()), &diagnostics);
        assert!(report.contains("| OT | 11 | 9 | 1 | 1 | 1 | 1 | skip card |"));
        assert!(report.contains("<summary>Image validation (warning records: 3)</summary>"));
        assert!(report.contains("<summary>Other diagnostics (warnings: 0, errors: 1)</summary>"));
        assert!(report.contains("Missing \\| &lt;Image&gt;"));
        assert!(report.contains("| OT | 42 | 43 | Missing \\| &lt;Image&gt; | card skipped |"));
        assert!(report.contains("https://example.test/42.jpg"));
        assert!(report.contains("HTTP 404<br>Not Found"));
        assert!(report.contains("| HEAD | https://example.test/43.jpg | 1 of 3 | 1 seconds | HTTP 503 Service Unavailable |"));
        assert!(report.contains("invalid type<br>missing race"));
        assert!(report.contains("| Suggestion | Check the source |"));
        assert_eq!(report.matches("### Failed image candidates").count(), 1);
        assert_eq!(report.matches("### Unresolved cards").count(), 1);
        assert!(
            report.find("## Diagnostics").unwrap() < report.find("### Unresolved cards").unwrap()
        );
        assert!(
            report.find("invalid type").unwrap() < report.find("### Unresolved cards").unwrap()
        );
        assert_eq!(report.matches("<details>").count(), 2);
        assert_eq!(report.matches("</details>").count(), 2);
        assert!(!report.contains("<details open"));

        let images_only = DiagnosticSnapshot::from_records(diagnostics.records[..3].to_vec());
        let report = build_report(&[&dataset], ReportScope::Full(&comparison()), &images_only);
        assert!(
            report.contains("Other diagnostics: no warnings or errors were recorded in this run.")
        );
        assert_eq!(report.matches("<details>").count(), 1);
        assert!(report.contains("<summary>Image validation (warning records: 3)</summary>"));
        assert!(!report.contains("<summary>Other diagnostics ("));

        let failure = build_failure_report(&diagnostics);
        assert!(failure.contains("Build failed; outputs are incomplete."));
        assert!(failure.contains("### Failed image candidates"));
        assert!(failure.contains("invalid type<br>missing race"));
        assert!(!failure.contains("First release"));
        assert_eq!(failure.matches("<details>").count(), 2);
        assert_eq!(failure.matches("</details>").count(), 2);
        assert!(!failure.contains("<details open"));
    }

    #[test]
    fn retains_non_image_diagnostics_and_unexpected_image_errors_in_full() {
        let diagnostics = DiagnosticSnapshot::from_records(vec![
            Diagnostic::warning("image.candidate-failed", "Image candidate check failed")
                .context("Image ID", 42)
                .reason("HTTP 404 Not Found"),
            Diagnostic::warning("card.skipped", "Card was skipped")
                .context("Environment", "OT")
                .context("Card ID", 99)
                .context("Name", "Invalid card")
                .reason("unsupported type or race bitmask"),
            Diagnostic::warning("database.row-skipped", "Database row was skipped")
                .context("Environment", "RD")
                .context("Row", 7)
                .reason("failed to decode SQLite row: invalid column type"),
            Diagnostic::warning(
                "resource.download-retry",
                "Resource download attempt failed",
            )
            .context("Resource", "ot cards database")
            .context("Attempt", "1 of 4")
            .reason("connection timed out")
            .suggestion("The download will be retried automatically"),
            Diagnostic::error("build.failed", "Build failed")
                .reason("failed to load resource\ncaused by: invalid data"),
            Diagnostic::error("image.candidate-failed", "Unexpected image error")
                .context("Custom field", "keep error context")
                .reason("keep error reason")
                .suggestion("keep error suggestion"),
        ]);
        let report = build_failure_report(&diagnostics);
        assert!(report.contains("<summary>Other diagnostics (warnings: 3, errors: 2)</summary>"));
        assert!(report.contains("<summary>Image validation (warning records: 1)</summary>"));
        for diagnostic in &diagnostics.records[1..] {
            assert_eq!(
                report
                    .matches(&format!("| Code | {} |", diagnostic.code))
                    .count(),
                1
            );
            assert!(report.contains(&escape_cell(&diagnostic.title)));
            for field in &diagnostic.context {
                assert!(report.contains(&format!(
                    "| {} | {} |",
                    field.label,
                    escape_cell(&field.value)
                )));
            }
            assert!(report.contains(&escape_cell(diagnostic.reason.as_deref().unwrap())));
            if let Some(suggestion) = &diagnostic.suggestion {
                assert!(report.contains(&escape_cell(suggestion)));
            }
        }
        assert_eq!(report.matches("HTTP 404 Not Found").count(), 1);
        assert_eq!(report.matches("<details>").count(), 2);
        assert!(!report.contains("<details open"));
    }
}
