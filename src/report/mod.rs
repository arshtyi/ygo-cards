mod console;
mod markdown;

use crate::cards::{DatasetReport, ImageCheckSummary};
use crate::formatting::{format_count, plural};

pub(crate) use console::{print_dataset_report, print_summary_report};
pub(crate) use markdown::write_summary_report;

pub(crate) const SUMMARY_REPORT: &str = "output/report.md";

pub(super) fn image_failure_action(summary: ImageCheckSummary) -> &'static str {
    if summary.skip_failures {
        "skip card"
    } else {
        "keep card with image = 0"
    }
}

pub(super) struct ReportTotals {
    pub(super) cards_written: usize,
    pub(super) cards_skipped: usize,
    pub(super) images: ImageCheckSummary,
    pub(super) image_failures: usize,
}

impl ReportTotals {
    pub(super) fn from_reports(reports: &[&DatasetReport]) -> Self {
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
