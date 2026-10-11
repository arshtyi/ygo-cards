mod images;
mod mappings;
mod rejection;
mod restrictions;
mod text;

use std::{fs, num::NonZeroUsize, path::Path};

use anyhow::{Context, Result};
use serde::Serialize;

pub(crate) mod ot;
pub(crate) mod rd;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GenerationOptions {
    pub(crate) limit: Option<NonZeroUsize>,
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
    pub(crate) network_errors: usize,
}
