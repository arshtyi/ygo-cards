use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ygo_cards::cards::{ImageFailure, ImageSummary, WriteReport};

use crate::latest::{LatestComparisonReport, LatestComparisonStatus};

pub(crate) const SUMMARY_REPORT: &str = "output/report.md";

pub(crate) fn print_write_report(report: &WriteReport) {
    println!(
        "wrote {} cards (skipped {}) -> {}",
        report.cards_written,
        report.cards_skipped,
        report.path.display()
    );

    for summary in &report.lf_summaries {
        println!(
            "  {} lf: forbidden={} limit={} semi={} unlimited={}",
            summary.label,
            summary.counts[0],
            summary.counts[1],
            summary.counts[2],
            summary.counts[3]
        );
    }

    print_image_summary(report.image_summary);
    print_image_failures(&report.image_failures);
}

fn print_image_summary(summary: ImageSummary) {
    if !summary.enabled {
        return;
    }

    println!(
        "  images: success={} failed={} primary={} alias={} checked_cards={} unique_found={} unique_missing={} cache_hits={} network_errors={}",
        summary.successful_cards(),
        summary.missing,
        summary.primary_found,
        summary.alias_found,
        summary.cards_checked,
        summary.unique_urls_found,
        summary.unique_urls_missing,
        summary.cache_hits,
        summary.network_errors,
    );
}

fn print_image_failures(failures: &[ImageFailure]) {
    if failures.is_empty() {
        return;
    }

    println!("  image failed cards:");
    for failure in failures {
        println!(
            "    {} id={} alias={} name={}",
            failure.environment, failure.id, failure.alias, failure.name
        );
    }
}

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

pub(crate) fn print_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
) {
    let totals = ReportTotals::from_reports(reports);

    println!("summary:");
    println!(
        "  cards: written={} skipped={}",
        totals.cards_written, totals.cards_skipped
    );
    if totals.images.enabled {
        println!(
            "  images: success={} failed={} network_errors={}",
            totals.images.successful_cards(),
            totals.image_failures,
            totals.images.network_errors
        );
    }

    for comparison in latest_comparisons {
        match &comparison.status {
            LatestComparisonStatus::Compared { previous_cards } => {
                println!(
                    "  {} new cards since latest: {} (previous={} current={})",
                    comparison.label,
                    comparison.added_cards.len(),
                    previous_cards,
                    comparison.current_cards
                );
            }
            LatestComparisonStatus::NotFound => {
                println!(
                    "  {} new cards since latest: skipped (latest asset not found)",
                    comparison.label
                );
            }
            LatestComparisonStatus::Unavailable(reason) => {
                println!(
                    "  {} new cards since latest: skipped ({})",
                    comparison.label, reason
                );
            }
        }
    }
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
        "| Image check | {} |\n\n",
        if totals.images.enabled {
            "enabled"
        } else {
            "disabled"
        }
    ));

    for environment_report in reports {
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
            report.push_str("| Metric | Value |\n");
            report.push_str("| --- | ---: |\n");
            report.push_str(&format!(
                "| Successful cards | {} |\n| Failed cards | {} |\n| Primary hits | {} |\n| Alias hits | {} |\n| Checked cards | {} |\n| Unique URLs found | {} |\n| Unique URLs missing | {} |\n| Cache hits | {} |\n| Network errors | {} |\n\n",
                environment_report.image_summary.successful_cards(),
                environment_report.image_failures.len(),
                environment_report.image_summary.primary_found,
                environment_report.image_summary.alias_found,
                environment_report.image_summary.cards_checked,
                environment_report.image_summary.unique_urls_found,
                environment_report.image_summary.unique_urls_missing,
                environment_report.image_summary.cache_hits,
                environment_report.image_summary.network_errors,
            ));
            append_image_failures(&mut report, &environment_report.image_failures);
        }
    }

    if totals.images.enabled {
        report.push_str("## Image Totals\n\n");
        report.push_str("| Metric | Value |\n");
        report.push_str("| --- | ---: |\n");
        report.push_str(&format!(
            "| Successful cards | {} |\n| Failed cards | {} |\n| Primary hits | {} |\n| Alias hits | {} |\n| Checked cards | {} |\n| Unique URLs found | {} |\n| Unique URLs missing | {} |\n| Cache hits | {} |\n| Network errors | {} |\n",
            totals.images.successful_cards(),
            totals.image_failures,
            totals.images.primary_found,
            totals.images.alias_found,
            totals.images.cards_checked,
            totals.images.unique_urls_found,
            totals.images.unique_urls_missing,
            totals.images.cache_hits,
            totals.images.network_errors
        ));
    }

    append_latest_comparison_report(&mut report, latest_comparisons);

    report
}

fn append_image_failures(report: &mut String, failures: &[ImageFailure]) {
    if failures.is_empty() {
        return;
    }

    report.push_str("#### Failed Image Cards\n\n");
    report.push_str("| Environment | ID | Alias | Name |\n");
    report.push_str("| --- | ---: | ---: | --- |\n");
    for failure in failures {
        report.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            failure.environment,
            failure.id,
            failure.alias,
            escape_markdown_cell(&failure.name)
        ));
    }
    report.push('\n');
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
                    report.push('\n');
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

struct ReportTotals {
    cards_written: usize,
    cards_skipped: usize,
    images: ImageSummary,
    image_failures: usize,
}

impl ReportTotals {
    fn from_reports(reports: &[&WriteReport]) -> Self {
        Self {
            cards_written: reports.iter().map(|report| report.cards_written).sum(),
            cards_skipped: reports.iter().map(|report| report.cards_skipped).sum(),
            images: reports.iter().map(|report| report.image_summary).sum(),
            image_failures: reports
                .iter()
                .map(|report| report.image_failures.len())
                .sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latest::CardSummary;

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
        assert!(report.contains("| 2 | New\\|Card | 怪兽/龙族/通常 |"));
    }
}
