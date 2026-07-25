mod console;
mod markdown;

use ygo_cards::cards::{ImageSummary, WriteReport};

pub(crate) use console::{print_summary_report, print_write_report};
pub(crate) use markdown::write_summary_report;

pub(crate) const SUMMARY_REPORT: &str = "output/report.md";

pub(super) fn image_failure_action(summary: ImageSummary) -> &'static str {
    if summary.skip_failures {
        "skip card"
    } else {
        "keep card with image = 0"
    }
}

pub(super) struct ReportTotals {
    pub(super) cards_written: usize,
    pub(super) cards_skipped: usize,
    pub(super) images: ImageSummary,
    pub(super) image_failures: usize,
}

impl ReportTotals {
    pub(super) fn from_reports(reports: &[&WriteReport]) -> Self {
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
