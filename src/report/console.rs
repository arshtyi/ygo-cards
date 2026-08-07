use std::fmt::Write;

use ygo_cards::cards::{ImageSummary, WriteReport};

use crate::latest::{LatestComparisonReport, LatestComparisonStatus};

use super::{ReportTotals, format_count, image_failure_action, plural};

pub(crate) fn print_write_report(report: &WriteReport) {
    println!("\n{}", render_write_report(report));
}

fn render_write_report(report: &WriteReport) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "{} dataset", report.label);
    let _ = writeln!(output, "  {:<16} {}", "Output", report.path.display());
    let _ = writeln!(
        output,
        "  {:<16} {} written, {} skipped",
        "Cards",
        format_count(report.cards_written),
        format_count(report.cards_skipped)
    );
    let _ = writeln!(
        output,
        "  {:<16} {}",
        "LF aliases",
        if report.lf_statistics_options.ignore_aliases {
            "ignored"
        } else {
            "included"
        }
    );

    if !report.lf_summaries.is_empty() {
        let _ = writeln!(
            output,
            "  Forbidden lists (forbidden / limited / semi-limited / unlimited)"
        );
        let width = report
            .lf_summaries
            .iter()
            .map(|summary| summary.label.len())
            .max()
            .unwrap_or_default();
        for summary in &report.lf_summaries {
            let _ = writeln!(
                output,
                "    {:<width$}  {:>5} / {:>5} / {:>5} / {:>5}",
                summary.label,
                format_count(summary.counts[0]),
                format_count(summary.counts[1]),
                format_count(summary.counts[2]),
                format_count(summary.counts[3])
            );
        }
    }

    append_image_summary(&mut output, report.image_summary);
    output.trim_end().to_string()
}

fn append_image_summary(output: &mut String, summary: ImageSummary) {
    if !summary.enabled {
        return;
    }

    let _ = writeln!(output, "  Images");
    let _ = writeln!(
        output,
        "    {:<14} {} of {} {}",
        "Resolved",
        format_count(summary.successful_cards()),
        format_count(summary.cards_checked),
        plural(summary.cards_checked, "card", "cards")
    );
    let _ = writeln!(
        output,
        "    {:<14} {} ({} {} skipped)",
        "Missing",
        format_count(summary.missing),
        format_count(summary.cards_skipped),
        plural(summary.cards_skipped, "card", "cards")
    );
    let _ = writeln!(
        output,
        "    {:<14} {} primary, {} alias",
        "Matches",
        format_count(summary.primary_found),
        format_count(summary.alias_found)
    );
    let _ = writeln!(
        output,
        "    {:<14} {} found, {} missing, {} cache hits",
        "URL checks",
        format_count(summary.unique_urls_found),
        format_count(summary.unique_urls_missing),
        format_count(summary.cache_hits)
    );
    let _ = writeln!(
        output,
        "    {:<14} {} ({} network {})",
        "Failure policy",
        image_failure_action(summary),
        format_count(summary.network_errors),
        plural(summary.network_errors, "error", "errors")
    );
}

pub(crate) fn print_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
) {
    println!("\n{}", render_summary_report(reports, latest_comparisons));
}

fn render_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
) -> String {
    let totals = ReportTotals::from_reports(reports);
    let mut output = String::from("Build summary\n");
    let _ = writeln!(
        output,
        "  {:<16} {} written, {} skipped",
        "Cards",
        format_count(totals.cards_written),
        format_count(totals.cards_skipped)
    );
    if totals.images.enabled {
        let _ = writeln!(
            output,
            "  {:<16} {} resolved, {} failed, {} network {}",
            "Images",
            format_count(totals.images.successful_cards()),
            format_count(totals.image_failures),
            format_count(totals.images.network_errors),
            plural(totals.images.network_errors, "error", "errors")
        );
    }

    if !latest_comparisons.is_empty() {
        let _ = writeln!(output, "  New since latest");
        let width = latest_comparisons
            .iter()
            .map(|comparison| comparison.label.len())
            .max()
            .unwrap_or_default();
        for comparison in latest_comparisons {
            match &comparison.status {
                LatestComparisonStatus::Compared { previous_cards } => {
                    let _ = writeln!(
                        output,
                        "    {:<width$}  {} new ({} -> {} cards)",
                        comparison.label,
                        format_count(comparison.added_cards.len()),
                        format_count(*previous_cards),
                        format_count(comparison.current_cards)
                    );
                }
                LatestComparisonStatus::NotFound => {
                    let _ = writeln!(
                        output,
                        "    {:<width$}  unavailable (no previous release)",
                        comparison.label
                    );
                }
                LatestComparisonStatus::Unavailable(_) => {
                    let _ = writeln!(
                        output,
                        "    {:<width$}  unavailable (see diagnostics)",
                        comparison.label
                    );
                }
            }
        }
    }

    output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ygo_cards::cards::{ImageSummary, LfStatisticsOptions};

    use super::*;

    #[test]
    fn renders_dataset_as_scannable_sections() {
        let report = WriteReport {
            label: "OT",
            path: PathBuf::from("output/ot.json"),
            cards_written: 14_947,
            cards_skipped: 1,
            lf_statistics_options: LfStatisticsOptions::default(),
            lf_summaries: Vec::new(),
            image_summary: ImageSummary {
                enabled: true,
                cards_checked: 11,
                primary_found: 9,
                alias_found: 1,
                missing: 1,
                cards_skipped: 1,
                unique_urls_found: 10,
                unique_urls_missing: 1,
                cache_hits: 2,
                network_errors: 1,
                skip_failures: true,
            },
            image_failures: Vec::new(),
        };

        let output = render_write_report(&report);

        assert!(output.starts_with("OT dataset\n"));
        assert!(output.contains("Cards            14,947 written, 1 skipped"));
        assert!(output.contains("Resolved       10 of 11 cards"));
        assert!(output.contains("Failure policy skip card (1 network error)"));
        assert!(!output.contains("success="));
    }
}
