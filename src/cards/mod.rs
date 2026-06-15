mod images;
mod masks;
mod text;

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
    pub cards_checked: usize,
    pub primary_found: usize,
    pub alias_found: usize,
    pub missing: usize,
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

#[derive(Debug, Clone)]
pub struct ImageFailure {
    pub environment: &'static str,
    pub id: i64,
    pub name: String,
    pub alias: i64,
}
