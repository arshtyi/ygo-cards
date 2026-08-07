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

pub(super) fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

pub(super) fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
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
