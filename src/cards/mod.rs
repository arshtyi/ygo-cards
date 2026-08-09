mod images;
mod mappings;
mod rejection;
mod restrictions;
mod text;

use std::{fs, iter::Sum, ops::AddAssign, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

pub(crate) mod ot;
pub(crate) mod rd;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GenerationOptions {
    pub(crate) check_images: bool,
    pub(crate) skip_image_failures: bool,
}

fn write_dataset(path: &Path, cards: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    crate::json::write_pretty_sorted(path, cards)
}

fn has_type(card_type: &[String], expected: &str) -> bool {
    card_type.iter().any(|name| name == expected)
}

struct CardCollection<T> {
    cards: Vec<T>,
    skipped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardKind {
    Monster,
    Spell,
    Trap,
}

impl CardKind {
    fn from_output_name(name: &str) -> Option<Self> {
        match name {
            "怪兽" => Some(Self::Monster),
            "魔法" => Some(Self::Spell),
            "陷阱" => Some(Self::Trap),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct DatasetReport {
    pub(crate) environment: crate::environment::Environment,
    pub(crate) path: std::path::PathBuf,
    pub(crate) cards_written: usize,
    pub(crate) cards_skipped: usize,
    pub(crate) image_summary: ImageCheckSummary,
    pub(crate) image_failures: Vec<ImageCheckFailure>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ImageCheckSummary {
    pub(crate) enabled: bool,
    pub(crate) skip_failures: bool,
    pub(crate) cards_checked: usize,
    pub(crate) primary_found: usize,
    pub(crate) alias_found: usize,
    pub(crate) missing: usize,
    pub(crate) cards_skipped: usize,
    pub(crate) unique_urls_found: usize,
    pub(crate) unique_urls_missing: usize,
    pub(crate) network_errors: usize,
    pub(crate) cache_hits: usize,
}

impl ImageCheckSummary {
    pub(crate) fn successful_cards(self) -> usize {
        self.primary_found + self.alias_found
    }
}

impl AddAssign for ImageCheckSummary {
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

impl Sum for ImageCheckSummary {
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
pub(crate) struct ImageCheckFailure {
    pub(crate) environment: crate::environment::Environment,
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) alias: i64,
    pub(crate) primary: FailedImageCheck,
    pub(crate) alias_check: Option<FailedImageCheck>,
    pub(crate) card_skipped: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct FailedImageCheck {
    pub(crate) image_id: i64,
    pub(crate) url: String,
    pub(crate) reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_image_summaries() {
        let total = [
            ImageCheckSummary {
                enabled: true,
                skip_failures: true,
                cards_checked: 2,
                primary_found: 1,
                missing: 1,
                cards_skipped: 1,
                ..ImageCheckSummary::default()
            },
            ImageCheckSummary {
                alias_found: 1,
                unique_urls_found: 1,
                unique_urls_missing: 2,
                network_errors: 1,
                cache_hits: 1,
                ..ImageCheckSummary::default()
            },
        ]
        .into_iter()
        .sum::<ImageCheckSummary>();

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
