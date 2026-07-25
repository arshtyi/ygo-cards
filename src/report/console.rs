use ygo_cards::cards::{ImageSummary, WriteReport};

use crate::latest::{LatestComparisonReport, LatestComparisonStatus};

use super::{ReportTotals, image_failure_action};

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
}

fn print_image_summary(summary: ImageSummary) {
    if !summary.enabled {
        return;
    }

    println!(
        "  images: success={} failed={} failure_action={} skipped={} primary={} alias={} checked_cards={} unique_found={} unique_missing={} cache_hits={} network_errors={}",
        summary.successful_cards(),
        summary.missing,
        image_failure_action(summary),
        summary.cards_skipped,
        summary.primary_found,
        summary.alias_found,
        summary.cards_checked,
        summary.unique_urls_found,
        summary.unique_urls_missing,
        summary.cache_hits,
        summary.network_errors,
    );
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
            "  images: success={} failed={} failure_action={} skipped={} network_errors={}",
            totals.images.successful_cards(),
            totals.image_failures,
            image_failure_action(totals.images),
            totals.images.cards_skipped,
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
            LatestComparisonStatus::NotFound | LatestComparisonStatus::Unavailable(_) => {
                println!(
                    "  {} new cards since latest: skipped (see build log)",
                    comparison.label
                );
            }
        }
    }
}
