mod images;
mod limit;
mod masks;
mod text;

use std::{iter::Sum, ops::AddAssign};

pub mod ot;
pub mod rd;

#[derive(Debug)]
pub struct WriteReport {
    pub label: &'static str,
    pub path: std::path::PathBuf,
    pub cards_written: usize,
    pub cards_skipped: usize,
    pub lf_summaries: Vec<LfSummary>,
    pub image_summary: ImageSummary,
    pub image_failures: Vec<ImageFailure>,
}

#[derive(Debug, Clone)]
pub struct LfSummary {
    pub label: &'static str,
    pub counts: [usize; 4],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ImageSummary {
    pub enabled: bool,
    pub skip_failures: bool,
    pub cards_checked: usize,
    pub primary_found: usize,
    pub alias_found: usize,
    pub missing: usize,
    pub cards_skipped: usize,
    pub unique_urls_found: usize,
    pub unique_urls_missing: usize,
    pub network_errors: usize,
    pub cache_hits: usize,
}

impl ImageSummary {
    pub fn successful_cards(self) -> usize {
        self.primary_found + self.alias_found
    }
}

impl AddAssign for ImageSummary {
    fn add_assign(&mut self, other: Self) {
        self.enabled |= other.enabled;
        self.skip_failures |= other.skip_failures;
        self.cards_checked += other.cards_checked;
        self.primary_found += other.primary_found;
        self.alias_found += other.alias_found;
        self.missing += other.missing;
        self.cards_skipped += other.cards_skipped;
        self.unique_urls_found += other.unique_urls_found;
        self.unique_urls_missing += other.unique_urls_missing;
        self.network_errors += other.network_errors;
        self.cache_hits += other.cache_hits;
    }
}

impl Sum for ImageSummary {
    fn sum<I>(iter: I) -> Self
    where
        I: Iterator<Item = Self>,
    {
        iter.fold(Self::default(), |mut total, summary| {
            total += summary;
            total
        })
    }
}

#[derive(Debug, Clone)]
pub struct ImageFailure {
    pub environment: &'static str,
    pub id: i64,
    pub name: String,
    pub alias: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_image_summaries() {
        let total = [
            ImageSummary {
                enabled: true,
                skip_failures: true,
                cards_checked: 2,
                primary_found: 1,
                missing: 1,
                cards_skipped: 1,
                ..ImageSummary::default()
            },
            ImageSummary {
                alias_found: 1,
                unique_urls_found: 1,
                unique_urls_missing: 2,
                network_errors: 1,
                cache_hits: 1,
                ..ImageSummary::default()
            },
        ]
        .into_iter()
        .sum::<ImageSummary>();

        assert!(total.enabled);
        assert!(total.skip_failures);
        assert_eq!(total.cards_checked, 2);
        assert_eq!(total.successful_cards(), 2);
        assert_eq!(total.missing, 1);
        assert_eq!(total.cards_skipped, 1);
        assert_eq!(total.unique_urls_found, 1);
        assert_eq!(total.unique_urls_missing, 2);
        assert_eq!(total.network_errors, 1);
        assert_eq!(total.cache_hits, 1);
    }
}
