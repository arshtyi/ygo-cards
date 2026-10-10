use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    cards::{DatasetReport, FailedImageCheck, ImageCheckFailure},
    diagnostics::{Diagnostic, DiagnosticSnapshot, Severity},
    latest::{LatestComparisonReport, LatestComparisonStatus, LatestReleaseComparison},
};

use super::{ReportTotals, format_count, image_failure_action, plural};

pub(crate) fn write_summary_report(
    reports: &[&DatasetReport],
    latest_comparison: &LatestReleaseComparison,
    diagnostics: &DiagnosticSnapshot,
    path: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }

    let text = build_summary_report(reports, latest_comparison, diagnostics);
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn build_summary_report(
    reports: &[&DatasetReport],
    latest_comparison: &LatestReleaseComparison,
    diagnostics: &DiagnosticSnapshot,
) -> String {
    let mut report = String::new();
    let totals = ReportTotals::from_reports(reports);

    report.push_str("# ygo-cards Card Data Report\n\n");
    append_release_summary(&mut report, reports, latest_comparison, &totals);
    append_image_report(&mut report, reports, &totals);
    append_diagnostics(&mut report, diagnostics);
    report
}

fn append_build_note(report: &mut String, datasets: usize, totals: &ReportTotals) {
    report.push_str("> [!NOTE]\n");
    report.push_str(&format!(
        "> Generated **{} {}** across **{} {}**; **{} {}** were skipped.\n",
        format_count(totals.cards_written),
        plural(totals.cards_written, "card", "cards"),
        format_count(datasets),
        plural(datasets, "dataset", "datasets"),
        format_count(totals.cards_skipped),
        plural(totals.cards_skipped, "card", "cards")
    ));
    if totals.images.enabled {
        report.push_str(&format!(
            "> Image validation resolved **{} of {} {}** with **{} {}**.\n\n",
            format_count(totals.images.successful_cards()),
            format_count(totals.images.cards_checked),
            plural(totals.images.cards_checked, "card", "cards"),
            format_count(totals.image_failures),
            plural(totals.image_failures, "failure", "failures")
        ));
    } else {
        report.push_str("> Image validation was not requested for this build.\n\n");
    }
}

fn append_overview(
    report: &mut String,
    reports: &[&DatasetReport],
    latest_comparisons: &[LatestComparisonReport],
    totals: &ReportTotals,
) {
    report.push_str("## At a Glance\n\n");
    report.push_str("| Dataset | Cards | Skipped | New since previous | Output |\n");
    report.push_str("| --- | ---: | ---: | ---: | --- |\n");
    for environment_report in reports {
        let latest = latest_comparisons
            .iter()
            .find(|comparison| comparison.environment == environment_report.environment)
            .map(latest_overview_value)
            .unwrap_or_else(|| String::from("Not checked"));
        report.push_str(&format!(
            "| **{}** | {} | {} | {} | `{}` |\n",
            escape_markdown_cell(environment_report.environment.label()),
            format_count(environment_report.cards_written),
            format_count(environment_report.cards_skipped),
            latest,
            escape_inline_code(&environment_report.path.display().to_string())
        ));
    }
    if reports.len() > 1 {
        let new_total = latest_comparisons
            .iter()
            .filter_map(|comparison| match comparison.status {
                LatestComparisonStatus::Compared { .. } => Some(comparison.added_cards),
                LatestComparisonStatus::NotFound => None,
            })
            .sum::<usize>();
        let all_compared = latest_comparisons.len() == reports.len()
            && latest_comparisons.iter().all(|comparison| {
                matches!(comparison.status, LatestComparisonStatus::Compared { .. })
            });
        let comparison_total = if all_compared {
            format!("**{}**", format_count(new_total))
        } else if latest_comparisons
            .iter()
            .any(|comparison| matches!(comparison.status, LatestComparisonStatus::Compared { .. }))
        {
            String::from("Partially available")
        } else {
            String::from("Unavailable")
        };
        report.push_str(&format!(
            "| **Total** | **{}** | **{}** | {} | — |\n",
            format_count(totals.cards_written),
            format_count(totals.cards_skipped),
            comparison_total
        ));
    }
    report.push('\n');
}

fn latest_overview_value(comparison: &LatestComparisonReport) -> String {
    match comparison.status {
        LatestComparisonStatus::Compared { .. } => format_count(comparison.added_cards),
        LatestComparisonStatus::NotFound => String::from("No baseline"),
    }
}

fn append_latest_comparison_report(
    report: &mut String,
    latest_comparison: &LatestReleaseComparison,
) {
    report.push_str("## Changes Since the Previous Release\n\n");

    if latest_comparison.datasets.is_empty() {
        report.push_str("No previous-release comparisons were requested.\n\n");
        return;
    }

    if let Some(release) = &latest_comparison.release {
        report.push_str(&format!(
            "Previous release: [{}]({}).\n\n",
            escape_markdown_cell(&release.tag),
            escape_markdown_url(&release.url)
        ));
    }

    for comparison in &latest_comparison.datasets {
        report.push_str(&format!("### {}\n\n", comparison.environment));
        match &comparison.status {
            LatestComparisonStatus::Compared { previous_cards } => {
                report.push_str(&format!(
                    "**{} added, {} modified, {} removed** ({} → {} cards).\n\n",
                    format_count(comparison.added_cards),
                    format_count(comparison.modified_cards),
                    format_count(comparison.removed_cards),
                    format_count(*previous_cards),
                    format_count(comparison.current_cards)
                ));
            }
            LatestComparisonStatus::NotFound => {
                report.push_str("No previous release; diff omitted.\n\n");
            }
        }
    }
}

fn append_release_summary(
    report: &mut String,
    reports: &[&DatasetReport],
    comparison: &LatestReleaseComparison,
    totals: &ReportTotals,
) {
    append_build_note(report, reports.len(), totals);
    append_overview(report, reports, &comparison.datasets, totals);
    append_latest_comparison_report(report, comparison);
}

pub(crate) fn write_release_notes(
    reports: &[&DatasetReport],
    comparison: &LatestReleaseComparison,
    path: &Path,
) -> Result<()> {
    let mut notes = String::new();
    append_release_summary(
        &mut notes,
        reports,
        comparison,
        &ReportTotals::from_reports(reports),
    );
    fs::write(path, notes).with_context(|| format!("failed to write {}", path.display()))
}

fn append_image_report(report: &mut String, reports: &[&DatasetReport], totals: &ReportTotals) {
    report.push_str("## Image Validation\n\n");
    if !totals.images.enabled {
        report.push_str("Image validation was disabled. Card IDs were used as image IDs.\n\n");
        return;
    }

    report.push_str(
        "| Dataset | Checked | Resolved | Missing | Cards skipped | Network errors | Policy |\n",
    );
    report.push_str("| --- | ---: | ---: | ---: | ---: | ---: | --- |\n");
    for environment_report in reports {
        let images = environment_report.image_summary;
        report.push_str(&format!(
            "| **{}** | {} | {} | {} | {} | {} | {} |\n",
            escape_markdown_cell(environment_report.environment.label()),
            format_count(images.cards_checked),
            format_count(images.successful_cards()),
            format_count(environment_report.image_failures.len()),
            format_count(images.cards_skipped),
            format_count(images.network_errors),
            image_failure_action(images)
        ));
    }
    if reports.len() > 1 {
        report.push_str(&format!(
            "| **Total** | **{}** | **{}** | **{}** | **{}** | **{}** | {} |\n",
            format_count(totals.images.cards_checked),
            format_count(totals.images.successful_cards()),
            format_count(totals.image_failures),
            format_count(totals.images.cards_skipped),
            format_count(totals.images.network_errors),
            image_failure_action(totals.images)
        ));
    }
    report.push('\n');

    begin_collapsed(report, "View image lookup statistics");
    report.push_str(
        "| Dataset | Primary matches | Alias matches | Unique URLs found | Unique URLs missing | Cache hits |\n",
    );
    report.push_str("| --- | ---: | ---: | ---: | ---: | ---: |\n");
    for environment_report in reports {
        let images = environment_report.image_summary;
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            escape_markdown_cell(environment_report.environment.label()),
            format_count(images.primary_found),
            format_count(images.alias_found),
            format_count(images.unique_urls_found),
            format_count(images.unique_urls_missing),
            format_count(images.cache_hits)
        ));
    }
    end_collapsed(report);

    append_image_failures(report, reports);
}

fn append_image_failures(report: &mut String, reports: &[&DatasetReport]) {
    let failures = reports
        .iter()
        .flat_map(|environment_report| environment_report.image_failures.iter())
        .collect::<Vec<_>>();
    if failures.is_empty() {
        report.push_str("> [!TIP]\n> Every checked card resolved to a valid image.\n\n");
        return;
    }

    begin_collapsed(
        report,
        &format!(
            "View {} {} with image failures",
            format_count(failures.len()),
            plural(failures.len(), "card", "cards")
        ),
    );
    report.push_str("| Dataset | Card ID | Alias | Name | Action |\n");
    report.push_str("| --- | ---: | ---: | --- | --- |\n");
    for failure in &failures {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            failure.environment,
            failure.id,
            failure.alias,
            escape_markdown_cell(&failure.name),
            failed_card_action(failure.card_skipped)
        ));
    }
    end_collapsed(report);

    let failed_checks = failures
        .iter()
        .map(|failure| 1 + usize::from(failure.alias_check.is_some()))
        .sum::<usize>();
    begin_collapsed(
        report,
        &format!(
            "View {} failed image candidate {}",
            format_count(failed_checks),
            plural(failed_checks, "check", "checks")
        ),
    );
    report.push_str("| Dataset | Card ID | Candidate | Image ID | Link | Failure reason |\n");
    report.push_str("| --- | ---: | --- | ---: | --- | --- |\n");
    for failure in failures {
        append_failed_image_check(report, failure, "primary", &failure.primary);
        if let Some(alias) = &failure.alias_check {
            append_failed_image_check(report, failure, "alias", alias);
        }
    }
    end_collapsed(report);
}

fn append_failed_image_check(
    report: &mut String,
    failure: &ImageCheckFailure,
    candidate: &str,
    check: &FailedImageCheck,
) {
    report.push_str(&format!(
        "| {} | {} | {} | {} | [open image]({}) | {} |\n",
        failure.environment,
        failure.id,
        candidate,
        check.image_id,
        escape_markdown_url(&check.url),
        escape_markdown_cell(&check.reason)
    ));
}

fn append_diagnostics(report: &mut String, diagnostics: &DiagnosticSnapshot) {
    report.push_str("## Build Diagnostics\n\n");
    if diagnostics.is_clean() {
        report.push_str("> [!TIP]\n> The build completed without warnings or errors.\n\n");
        return;
    }

    report.push_str(&format!(
        "> [!{}]\n> The build recorded **{} {}** and **{} {}**. Details are grouped below.\n\n",
        if diagnostics.errors() > 0 {
            "CAUTION"
        } else {
            "WARNING"
        },
        format_count(diagnostics.warnings()),
        plural(diagnostics.warnings(), "warning", "warnings"),
        format_count(diagnostics.errors()),
        plural(diagnostics.errors(), "error", "errors")
    ));

    let mut grouped = BTreeMap::<(u8, &str, &str), usize>::new();
    for diagnostic in diagnostics.records() {
        let order = match diagnostic.severity() {
            Severity::Warning => 0,
            Severity::Error => 1,
        };
        *grouped
            .entry((order, diagnostic.code(), diagnostic.title()))
            .or_default() += 1;
    }

    report.push_str("| Severity | Code | Description | Count |\n");
    report.push_str("| --- | --- | --- | ---: |\n");
    for ((order, code, title), count) in grouped {
        report.push_str(&format!(
            "| {} | `{}` | {} | {} |\n",
            if order == 0 { "Warning" } else { "Error" },
            escape_inline_code(code),
            escape_markdown_cell(title),
            format_count(count)
        ));
    }
    report.push('\n');

    let detailed_records = diagnostics
        .records()
        .iter()
        .enumerate()
        .filter(|(_, diagnostic)| !has_dedicated_report(diagnostic))
        .collect::<Vec<_>>();
    let dedicated_records = diagnostics.records().len() - detailed_records.len();
    if dedicated_records > 0 {
        report.push_str(&format!(
            "{} {} {} already represented in the dedicated sections above and not duplicated here.\n\n",
            format_count(dedicated_records),
            plural(dedicated_records, "diagnostic record", "diagnostic records"),
            plural(dedicated_records, "is", "are")
        ));
    }
    if detailed_records.is_empty() {
        return;
    }

    begin_collapsed(
        report,
        &format!(
            "View {} {}",
            format_count(detailed_records.len()),
            plural(
                detailed_records.len(),
                "diagnostic record",
                "diagnostic records"
            )
        ),
    );
    for (index, diagnostic) in detailed_records {
        append_diagnostic_record(report, index + 1, diagnostic);
    }
    end_collapsed(report);
}

fn has_dedicated_report(diagnostic: &Diagnostic) -> bool {
    matches!(
        diagnostic.code(),
        "image.candidate-failed" | "image.unresolved" | "release.comparison-skipped"
    )
}

fn append_diagnostic_record(report: &mut String, number: usize, diagnostic: &Diagnostic) {
    report.push_str(&format!(
        "#### {} {number:03} · {}\n\n",
        diagnostic.severity().label(),
        escape_markdown_cell(diagnostic.title())
    ));
    report.push_str("| Field | Value |\n");
    report.push_str("| --- | --- |\n");
    report.push_str(&format!(
        "| Code | `{}` |\n",
        escape_inline_code(diagnostic.code())
    ));
    for field in diagnostic.fields() {
        report.push_str(&format!(
            "| {} | {} |\n",
            escape_markdown_cell(field.label()),
            escape_markdown_cell(field.value())
        ));
    }
    if let Some(reason) = diagnostic.reason_text() {
        report.push_str(&format!("| Reason | {} |\n", escape_markdown_cell(reason)));
    }
    if let Some(suggestion) = diagnostic.suggestion_text() {
        report.push_str(&format!(
            "| Suggestion | {} |\n",
            escape_markdown_cell(suggestion)
        ));
    }
    report.push('\n');
}

fn failed_card_action(card_skipped: bool) -> &'static str {
    if card_skipped {
        "skipped"
    } else {
        "kept with image 0"
    }
}

fn begin_collapsed(report: &mut String, summary: &str) {
    report.push_str("<details>\n");
    report.push_str(&format!("<summary>{}</summary>\n\n", escape_html(summary)));
}

fn end_collapsed(report: &mut String) {
    report.push_str("\n</details>\n\n");
}

fn escape_markdown_cell(text: &str) -> String {
    escape_html(text)
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('\n', "<br>")
}

fn escape_inline_code(text: &str) -> String {
    escape_html(text).replace('`', "&#96;")
}

fn escape_markdown_url(url: &str) -> String {
    url.replace('(', "%28").replace(')', "%29")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        cards::ImageCheckSummary, diagnostics::Diagnostic, environment::Environment,
        latest::ReleaseReference,
    };

    fn comparison(datasets: Vec<LatestComparisonReport>) -> LatestReleaseComparison {
        LatestReleaseComparison {
            release: None,
            datasets,
        }
    }

    fn report_with_image_failure() -> DatasetReport {
        DatasetReport {
            environment: Environment::Ot,
            path: PathBuf::from("output/ot.json"),
            cards_written: 9,
            cards_skipped: 1,
            image_summary: ImageCheckSummary {
                enabled: true,
                skip_failures: true,
                cards_checked: 10,
                primary_found: 9,
                missing: 1,
                cards_skipped: 1,
                ..ImageCheckSummary::default()
            },
            image_failures: vec![ImageCheckFailure {
                environment: Environment::Ot,
                id: 1,
                name: String::from("Missing | <Image>"),
                alias: 2,
                primary: FailedImageCheck {
                    image_id: 1,
                    url: String::from("https://example.test/1.jpg"),
                    reason: String::from("HTTP 404 Not Found"),
                },
                alias_check: Some(FailedImageCheck {
                    image_id: 2,
                    url: String::from("https://example.test/2.jpg"),
                    reason: String::from("request | timed out"),
                }),
                card_skipped: true,
            }],
        }
    }

    #[test]
    fn formats_large_counts() {
        assert_eq!(format_count(12_345_678), "12,345,678");
    }

    #[test]
    fn puts_release_changes_before_operational_details() {
        let environment_report = report_with_image_failure();
        let comparisons = LatestReleaseComparison {
            release: Some(ReleaseReference {
                tag: String::from("0.0.1"),
                url: String::from("https://github.com/arshtyi/ygo-cards/releases/tag/0.0.1"),
            }),
            datasets: vec![LatestComparisonReport {
                environment: Environment::Ot,
                current_cards: 9,
                status: LatestComparisonStatus::Compared { previous_cards: 8 },
                added_cards: 1,
                modified_cards: 2,
                removed_cards: 0,
                diff: String::from("--- a/ot.json\n+++ b/ot.json\n@@ -1 +1 @@\n-old\n+new\n"),
            }],
        };

        let report = build_summary_report(
            &[&environment_report],
            &comparisons,
            &DiagnosticSnapshot::default(),
        );

        assert!(report.starts_with("# ygo-cards Card Data Report\n\n> [!NOTE]"));
        assert!(report.contains("| **OT** | 9 | 1 | 1 | `output/ot.json` |"));
        assert!(report.contains(
            "Previous release: [0.0.1](https://github.com/arshtyi/ygo-cards/releases/tag/0.0.1)."
        ));
        assert!(!report.contains("releases/latest/download"));
        assert!(report.contains("**1 added, 2 modified, 0 removed** (8 → 9 cards)."));
        assert!(!report.contains("```diff"));
        assert!(!report.contains(&comparisons.datasets[0].diff));
        let changes = report
            .split("## Changes Since the Previous Release")
            .nth(1)
            .unwrap()
            .split("## Image Validation")
            .next()
            .unwrap();
        assert!(!changes.contains("<details>"));
        assert!(
            report
                .find("## Changes Since the Previous Release")
                .unwrap()
                < report.find("## Image Validation").unwrap()
        );
        assert!(report.contains("> The build completed without warnings or errors."));
        assert!(!report.contains("output/build.log"));
    }

    #[test]
    fn labels_the_total_unavailable_when_no_comparison_succeeded() {
        let ot_report = report_with_image_failure();
        let mut rd_report = report_with_image_failure();
        rd_report.environment = Environment::Rd;
        rd_report.path = PathBuf::from("output/rd.json");
        rd_report.cards_written = 3;
        rd_report.cards_skipped = 0;

        let reports = [&ot_report, &rd_report];
        let totals = ReportTotals::from_reports(&reports);
        let mut output = String::new();
        append_overview(&mut output, &reports, &[], &totals);

        assert!(output.contains("| **Total** | **12** | **1** | Unavailable | — |"));
    }

    #[test]
    fn report_and_release_notes_never_embed_diff_contents() {
        let environment_report = report_with_image_failure();
        for status in [
            LatestComparisonStatus::NotFound,
            LatestComparisonStatus::Compared { previous_cards: 9 },
        ] {
            let comparisons = comparison(vec![LatestComparisonReport {
                environment: Environment::Ot,
                current_cards: 9,
                status,
                added_cards: 0,
                modified_cards: 1,
                removed_cards: 0,
                diff: format!(
                    "--- a/ot.json\n+++ b/ot.json\n@@ -1 +1 @@\n-old\n+{}\n",
                    "内容".repeat(20_000)
                ),
            }]);
            let mut notes = String::new();
            append_release_summary(
                &mut notes,
                &[&environment_report],
                &comparisons,
                &ReportTotals::from_reports(&[&environment_report]),
            );
            assert!(notes.len() < 2_000);
            assert!(!notes.contains("```diff"));
            assert!(!notes.contains("<details>"));
            assert!(!notes.contains("## Image Validation"));
            assert!(!notes.contains("内容"));
            assert!(!notes.contains("ot.diff"));
            assert!(!notes.contains("preview"));
            let report = build_summary_report(
                &[&environment_report],
                &comparisons,
                &DiagnosticSnapshot::default(),
            );
            assert!(!report.contains("```diff"));
            assert!(!report.contains("内容"));
            let mut without_diff = comparisons;
            without_diff.datasets[0].diff.clear();
            let mut notes_without_diff = String::new();
            append_release_summary(
                &mut notes_without_diff,
                &[&environment_report],
                &without_diff,
                &ReportTotals::from_reports(&[&environment_report]),
            );
            assert_eq!(notes, notes_without_diff);
            assert_eq!(
                report,
                build_summary_report(
                    &[&environment_report],
                    &without_diff,
                    &DiagnosticSnapshot::default()
                )
            );
            match without_diff.datasets[0].status {
                LatestComparisonStatus::NotFound => {
                    assert!(notes.contains("No previous release; diff omitted."));
                    assert!(!notes.contains("modified"));
                }
                LatestComparisonStatus::Compared { .. } => {
                    assert!(notes.contains("**0 added, 1 modified, 0 removed**"))
                }
            }
        }
    }

    #[test]
    fn renders_image_and_diagnostic_details_safely() {
        let environment_report = report_with_image_failure();
        let diagnostics = DiagnosticSnapshot::from_records(vec![
            Diagnostic::warning("image.candidate-failed", "Image candidate check failed")
                .context("Card", "Missing | <Image>")
                .reason("HTTP 404\nNot Found"),
            Diagnostic::warning("image.candidate-failed", "Image candidate check failed")
                .context("Card", "Alias")
                .reason("timeout"),
            Diagnostic::warning("card.skipped", "Card was skipped")
                .context("Card", "Broken | Card")
                .reason("invalid type\nmissing race"),
        ]);

        let report = build_summary_report(
            &[&environment_report],
            &comparison(Vec::new()),
            &diagnostics,
        );

        assert!(report.contains("| **OT** | 10 | 9 | 1 | 1 | 0 | skip card |"));
        assert!(report.contains("Missing \\| &lt;Image&gt;"));
        assert!(report.contains("[open image](https://example.test/1.jpg)"));
        assert!(
            report.contains(
                "| Warning | `image.candidate-failed` | Image candidate check failed | 2 |"
            )
        );
        assert!(report.contains("2 diagnostic records are already represented"));
        assert!(report.contains("<summary>View 1 diagnostic record</summary>"));
        assert!(report.contains("invalid type<br>missing race"));
        assert_eq!(
            report.matches("<details>").count(),
            report.matches("</details>").count()
        );
        assert!(!report.contains("````log"));
    }
}
