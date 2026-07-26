use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ygo_cards::cards::{FailedImageCheck, ImageFailure, WriteReport};

use crate::latest::{LatestComparisonReport, LatestComparisonStatus};

use super::{ReportTotals, image_failure_action};

pub(crate) fn write_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
    path: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }

    let text = build_summary_report(reports, latest_comparisons);
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn build_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
) -> String {
    let mut report = String::new();
    let totals = ReportTotals::from_reports(reports);

    report.push_str("# ygo-cards Build Report\n\n");
    report.push_str("## Overview\n\n");
    report.push_str("| Metric | Value |\n");
    report.push_str("| --- | ---: |\n");
    report.push_str(&format!("| Cards written | {} |\n", totals.cards_written));
    report.push_str(&format!("| Cards skipped | {} |\n", totals.cards_skipped));
    report.push_str(&format!(
        "| Build log | `{}` |\n",
        ygo_cards::diagnostics::BUILD_LOG_PATH
    ));
    report.push_str(&format!(
        "| Image check | {} |\n",
        if totals.images.enabled {
            "enabled"
        } else {
            "disabled"
        }
    ));
    if totals.images.enabled {
        report.push_str(&format!(
            "| On image failure | {} |\n| Cards skipped after image failure | {} |\n",
            image_failure_action(totals.images),
            totals.images.cards_skipped
        ));
    }
    report.push('\n');

    for environment_report in reports {
        append_environment_report(&mut report, environment_report);
    }

    if totals.images.enabled {
        append_image_totals(&mut report, &totals);
    }

    append_latest_comparison_report(&mut report, latest_comparisons);
    report
}

fn append_environment_report(report: &mut String, environment_report: &WriteReport) {
    report.push_str(&format!("## {}\n\n", environment_report.label));
    report.push_str("| Metric | Value |\n");
    report.push_str("| --- | ---: |\n");
    report.push_str(&format!(
        "| Output | `{}` |\n| Cards written | {} |\n| Cards skipped | {} |\n\n",
        environment_report.path.display(),
        environment_report.cards_written,
        environment_report.cards_skipped
    ));

    report.push_str("### Forbidden Lists\n\n");
    report.push_str("| List | Forbidden | Limit | Semi | Unlimited |\n");
    report.push_str("| --- | ---: | ---: | ---: | ---: |\n");
    for summary in &environment_report.lf_summaries {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            summary.label,
            summary.counts[0],
            summary.counts[1],
            summary.counts[2],
            summary.counts[3]
        ));
    }
    report.push('\n');

    if environment_report.image_summary.enabled {
        report.push_str("### Images\n\n");
        begin_collapsed(report, "Image statistics");
        report.push_str("| Metric | Value |\n");
        report.push_str("| --- | ---: |\n");
        report.push_str(&format!(
            "| Successful cards | {} |\n| Failed cards | {} |\n| On failure | {} |\n| Cards skipped after image failure | {} |\n| Primary hits | {} |\n| Alias hits | {} |\n| Checked cards | {} |\n| Unique URLs found | {} |\n| Unique URLs missing | {} |\n| Cache hits | {} |\n| Network errors | {} |\n",
            environment_report.image_summary.successful_cards(),
            environment_report.image_failures.len(),
            image_failure_action(environment_report.image_summary),
            environment_report.image_summary.cards_skipped,
            environment_report.image_summary.primary_found,
            environment_report.image_summary.alias_found,
            environment_report.image_summary.cards_checked,
            environment_report.image_summary.unique_urls_found,
            environment_report.image_summary.unique_urls_missing,
            environment_report.image_summary.cache_hits,
            environment_report.image_summary.network_errors,
        ));
        end_collapsed(report);
        append_image_failures(report, &environment_report.image_failures);
    }
}

fn append_image_totals(report: &mut String, totals: &ReportTotals) {
    report.push_str("## Image Totals\n\n");
    begin_collapsed(report, "Detailed image totals");
    report.push_str("| Metric | Value |\n");
    report.push_str("| --- | ---: |\n");
    report.push_str(&format!(
        "| Successful cards | {} |\n| Failed cards | {} |\n| On failure | {} |\n| Cards skipped after image failure | {} |\n| Primary hits | {} |\n| Alias hits | {} |\n| Checked cards | {} |\n| Unique URLs found | {} |\n| Unique URLs missing | {} |\n| Cache hits | {} |\n| Network errors | {} |\n",
        totals.images.successful_cards(),
        totals.image_failures,
        image_failure_action(totals.images),
        totals.images.cards_skipped,
        totals.images.primary_found,
        totals.images.alias_found,
        totals.images.cards_checked,
        totals.images.unique_urls_found,
        totals.images.unique_urls_missing,
        totals.images.cache_hits,
        totals.images.network_errors
    ));
    end_collapsed(report);
}

fn append_image_failures(report: &mut String, failures: &[ImageFailure]) {
    if failures.is_empty() {
        return;
    }

    report.push_str("#### Image Failures\n\n");
    begin_collapsed(report, &format!("Failed image cards ({})", failures.len()));
    report.push_str("| Environment | Card ID | Alias | Name | Action |\n");
    report.push_str("| --- | ---: | ---: | --- | --- |\n");
    for failure in failures {
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
        &format!("Failed image candidate checks ({failed_checks})"),
    );
    report.push_str(
        "| Environment | Card ID | Candidate | Image ID | URL | Failure reason |\n",
    );
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
    failure: &ImageFailure,
    candidate: &str,
    check: &FailedImageCheck,
) {
    report.push_str(&format!(
        "| {} | {} | {} | {} | {} | {} |\n",
        failure.environment,
        failure.id,
        candidate,
        check.image_id,
        escape_markdown_cell(&check.url),
        escape_markdown_cell(&check.reason)
    ));
}

fn failed_card_action(card_skipped: bool) -> &'static str {
    if card_skipped {
        "skipped"
    } else {
        "kept with image = 0"
    }
}

fn append_latest_comparison_report(
    report: &mut String,
    latest_comparisons: &[LatestComparisonReport],
) {
    report.push_str("## New Cards Since Latest Release\n\n");

    for comparison in latest_comparisons {
        report.push_str(&format!("### {}\n\n", comparison.label));
        report.push_str(&format!(
            "Compared against [{}]({}).\n\n",
            comparison.latest_url, comparison.latest_url
        ));

        match &comparison.status {
            LatestComparisonStatus::Compared { previous_cards } => {
                report.push_str("| Metric | Value |\n");
                report.push_str("| --- | ---: |\n");
                report.push_str(&format!("| Previous cards | {} |\n", previous_cards));
                report.push_str(&format!(
                    "| Current cards | {} |\n",
                    comparison.current_cards
                ));
                report.push_str(&format!(
                    "| New cards | {} |\n\n",
                    comparison.added_cards.len()
                ));

                if comparison.added_cards.is_empty() {
                    report.push_str("No new cards.\n\n");
                } else {
                    begin_collapsed(
                        report,
                        &format!("New card details ({})", comparison.added_cards.len()),
                    );
                    report.push_str("| ID | Name | Type |\n");
                    report.push_str("| ---: | --- | --- |\n");
                    for card in &comparison.added_cards {
                        report.push_str(&format!(
                            "| {} | {} | {} |\n",
                            card.id,
                            escape_markdown_cell(&card.name),
                            escape_markdown_cell(&card_type_display(&card.card_type))
                        ));
                    }
                    end_collapsed(report);
                }
            }
            LatestComparisonStatus::NotFound => {
                report.push_str("Previous latest file was not found; comparison skipped.\n\n");
            }
            LatestComparisonStatus::Unavailable(reason) => {
                report.push_str(&format!(
                    "Previous latest file could not be compared: {}.\n\n",
                    escape_markdown_cell(reason)
                ));
            }
        }
    }
}

fn begin_collapsed(report: &mut String, summary: &str) {
    report.push_str("<details>\n");
    report.push_str(&format!("<summary>{summary}</summary>\n\n"));
}

fn end_collapsed(report: &mut String) {
    report.push_str("\n</details>\n\n");
}

fn escape_markdown_cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "<br>")
}

fn card_type_display(card_type: &[String]) -> String {
    if card_type.is_empty() {
        String::from("-")
    } else {
        card_type.join("/")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::latest::CardSummary;
    use ygo_cards::cards::ImageSummary;

    fn card(id: i64, name: &str, card_type: &[&str]) -> CardSummary {
        CardSummary {
            id,
            name: name.to_string(),
            card_type: card_type.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn formats_card_types_with_slashes() {
        assert_eq!(
            card_type_display(&["怪兽".to_string(), "龙族".to_string(), "通常".to_string()]),
            "怪兽/龙族/通常"
        );
        assert_eq!(card_type_display(&[]), "-");
    }

    #[test]
    fn appends_added_cards_to_latest_comparison_report() {
        let mut report = String::new();
        append_latest_comparison_report(
            &mut report,
            &[LatestComparisonReport {
                label: "OT",
                latest_url: ygo_cards::urls::urls()
                    .unwrap()
                    .latest_json_url("OT")
                    .unwrap()
                    .to_string(),
                current_cards: 2,
                status: LatestComparisonStatus::Compared { previous_cards: 1 },
                added_cards: vec![card(2, "New|Card", &["怪兽", "龙族", "通常"])],
            }],
        );

        assert!(report.contains("## New Cards Since Latest Release"));
        assert!(report.contains("| New cards | 1 |"));
        assert!(report.contains("<summary>New card details (1)</summary>"));
        assert!(report.contains("| 2 | New\\|Card | 怪兽/龙族/通常 |"));
        assert_eq!(report.matches("<details>").count(), 1);
        assert_eq!(report.matches("</details>").count(), 1);
    }

    #[test]
    fn reports_detailed_image_failures_and_policy() {
        let environment_report = WriteReport {
            label: "OT",
            path: PathBuf::from("output/ot.json"),
            cards_written: 9,
            cards_skipped: 1,
            lf_summaries: Vec::new(),
            image_summary: ImageSummary {
                enabled: true,
                skip_failures: true,
                cards_checked: 10,
                primary_found: 9,
                missing: 1,
                cards_skipped: 1,
                ..ImageSummary::default()
            },
            image_failures: vec![ImageFailure {
                environment: "OT",
                id: 1,
                name: String::from("Missing | Image"),
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
        };

        let report = build_summary_report(&[&environment_report], &[]);

        assert!(report.contains("| On image failure | skip card |"));
        assert!(report.contains("| On failure | skip card |"));
        assert!(report.contains("| Cards skipped after image failure | 1 |"));
        assert!(report.contains("| Build log | `output/build.log` |"));
        assert!(report.contains("<summary>Image statistics</summary>"));
        assert!(report.contains("<summary>Detailed image totals</summary>"));
        assert!(report.contains("<summary>Failed image cards (1)</summary>"));
        assert!(report.contains("<summary>Failed image candidate checks (2)</summary>"));
        assert!(report.contains("| OT | 1 | 2 | Missing \\| Image | skipped |"));
        assert!(report.contains(
            "| OT | 1 | primary | 1 | https://example.test/1.jpg | HTTP 404 Not Found |"
        ));
        assert!(report.contains(
            "| OT | 1 | alias | 2 | https://example.test/2.jpg | request \\| timed out |"
        ));
        assert_eq!(report.matches("<details>").count(), 4);
        assert_eq!(report.matches("</details>").count(), 4);

        let mut default_report = environment_report;
        default_report.cards_written = 10;
        default_report.cards_skipped = 0;
        default_report.image_summary.skip_failures = false;
        default_report.image_summary.cards_skipped = 0;
        default_report.image_failures[0].card_skipped = false;
        let report = build_summary_report(&[&default_report], &[]);

        assert!(report.contains("| On image failure | keep card with image = 0 |"));
        assert!(report.contains("| Cards skipped after image failure | 0 |"));
        assert!(report.contains("| OT | 1 | 2 | Missing \\| Image | kept with image = 0 |"));
    }
}
