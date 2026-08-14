use std::{collections::HashSet, fs, path::Path, time::Duration};

use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::Deserialize;

use crate::{
    cards::DatasetReport,
    diagnostics::{self, Diagnostic},
    endpoints::{self, Endpoints},
    environment::Environment,
};

pub(crate) fn compare_latest_release(
    reports: &[&DatasetReport],
) -> Result<LatestReleaseComparison> {
    let endpoints = endpoints::endpoints()?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build latest release HTTP client")?;

    let datasets = reports
        .iter()
        .map(|report| compare_latest_release_file(&client, endpoints, report))
        .collect::<Result<Vec<_>>>()?;
    let release = datasets
        .iter()
        .any(|comparison| matches!(comparison.status, LatestComparisonStatus::Compared { .. }))
        .then(|| resolve_latest_release(&client, endpoints.latest_release_url()))
        .flatten();

    Ok(LatestReleaseComparison { release, datasets })
}

fn compare_latest_release_file(
    client: &reqwest::blocking::Client,
    endpoints: &Endpoints,
    report: &DatasetReport,
) -> Result<LatestComparisonReport> {
    let latest_url = endpoints
        .published_dataset_url(report.environment)
        .to_string();
    let current_cards = read_card_summaries(&report.path)
        .with_context(|| format!("failed to read current {} cards", report.environment))?;

    let (status, added_cards) = match fetch_latest_card_summaries(client, &latest_url) {
        LatestCardsFetch::Cards(previous_cards) => {
            let added_cards = find_added_cards(&current_cards, &previous_cards);
            (
                LatestComparisonStatus::Compared {
                    previous_cards: previous_cards.len(),
                },
                added_cards,
            )
        }
        LatestCardsFetch::NotFound => {
            diagnostics::record(
                Diagnostic::warning(
                    "release.comparison-skipped",
                    "Latest-release comparison was skipped",
                )
                .context("Environment", report.environment)
                .context("URL", &latest_url)
                .reason("HTTP 404 Not Found")
                .suggestion("This is expected before the first published release"),
            );
            (LatestComparisonStatus::NotFound, Vec::new())
        }
        LatestCardsFetch::Unavailable(reason) => {
            diagnostics::record(
                Diagnostic::warning(
                    "release.comparison-skipped",
                    "Latest-release comparison was skipped",
                )
                .context("Environment", report.environment)
                .context("URL", &latest_url)
                .reason(&reason)
                .suggestion(
                    "The generated dataset is still valid, but new-card counts are unavailable",
                ),
            );
            (LatestComparisonStatus::Unavailable(reason), Vec::new())
        }
    };

    Ok(LatestComparisonReport {
        environment: report.environment,
        current_cards: current_cards.len(),
        status,
        added_cards,
    })
}

fn resolve_latest_release(
    client: &reqwest::blocking::Client,
    latest_release_url: &str,
) -> Option<ReleaseReference> {
    let response = match client.head(latest_release_url).send() {
        Ok(response) => response,
        Err(error) => {
            record_release_reference_warning(
                latest_release_url,
                format!("request failed: {:#}", anyhow::Error::new(error)),
            );
            return None;
        }
    };

    if !response.status().is_success() {
        record_release_reference_warning(latest_release_url, format!("HTTP {}", response.status()));
        return None;
    }

    let mut url = response.url().clone();
    let Some(tag) = release_tag_from_path(url.path()) else {
        record_release_reference_warning(
            latest_release_url,
            format!("redirect resolved to an unexpected URL: {url}"),
        );
        return None;
    };
    let tag = tag.to_string();
    url.set_query(None);
    url.set_fragment(None);

    Some(ReleaseReference {
        tag,
        url: url.to_string(),
    })
}

fn release_tag_from_path(path: &str) -> Option<&str> {
    let (_, tag) = path.split_once("/releases/tag/")?;
    (!tag.is_empty()).then_some(tag)
}

fn record_release_reference_warning(url: &str, reason: String) {
    diagnostics::record(
        Diagnostic::warning(
            "release.reference-unavailable",
            "Previous-release link could not be resolved",
        )
        .context("URL", url)
        .reason(reason)
        .suggestion("The dataset comparison is still valid, but the report will omit the link"),
    );
}

fn fetch_latest_card_summaries(client: &reqwest::blocking::Client, url: &str) -> LatestCardsFetch {
    let response = match client.get(url).send() {
        Ok(response) => response,
        Err(error) => {
            return LatestCardsFetch::Unavailable(format!(
                "download failed: {:#}",
                anyhow::Error::new(error)
            ));
        }
    };

    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return LatestCardsFetch::NotFound,
        status => return LatestCardsFetch::Unavailable(format!("HTTP {status}")),
    }

    let text = match response.text() {
        Ok(text) => text,
        Err(error) => {
            return LatestCardsFetch::Unavailable(format!(
                "read failed: {:#}",
                anyhow::Error::new(error)
            ));
        }
    };

    match parse_card_summaries(&text) {
        Ok(cards) => LatestCardsFetch::Cards(cards),
        Err(error) => LatestCardsFetch::Unavailable(format!("invalid JSON: {error:#}")),
    }
}

fn read_card_summaries(path: &Path) -> Result<Vec<CardSummary>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read cards JSON {}", path.display()))?;
    parse_card_summaries(&text)
}

fn parse_card_summaries(text: &str) -> Result<Vec<CardSummary>> {
    serde_json::from_str(text).context("failed to parse card summaries")
}

fn find_added_cards(
    current_cards: &[CardSummary],
    previous_cards: &[CardSummary],
) -> Vec<CardSummary> {
    let previous_ids = previous_cards
        .iter()
        .map(|card| card.id)
        .collect::<HashSet<_>>();

    current_cards
        .iter()
        .filter(|card| !previous_ids.contains(&card.id))
        .cloned()
        .collect()
}

#[derive(Debug)]
pub(crate) struct LatestReleaseComparison {
    pub(crate) release: Option<ReleaseReference>,
    pub(crate) datasets: Vec<LatestComparisonReport>,
}

#[derive(Debug)]
pub(crate) struct ReleaseReference {
    pub(crate) tag: String,
    pub(crate) url: String,
}

#[derive(Debug)]
pub(crate) struct LatestComparisonReport {
    pub(crate) environment: Environment,
    pub(crate) current_cards: usize,
    pub(crate) status: LatestComparisonStatus,
    pub(crate) added_cards: Vec<CardSummary>,
}

#[derive(Debug)]
pub(crate) enum LatestComparisonStatus {
    Compared { previous_cards: usize },
    NotFound,
    Unavailable(String),
}

enum LatestCardsFetch {
    Cards(Vec<CardSummary>),
    NotFound,
    Unavailable(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct CardSummary {
    pub(crate) id: i64,
    pub(crate) name: String,
    #[serde(default, rename = "type")]
    pub(crate) card_type: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: i64, name: &str, card_type: &[&str]) -> CardSummary {
        CardSummary {
            id,
            name: name.to_string(),
            card_type: card_type.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn parses_card_summaries_from_generated_json_shape() {
        let cards = parse_card_summaries(
            r#"[
                {"id":89631139,"name":"Blue-Eyes White Dragon","type":["怪兽","龙族","通常"],"atk":3000},
                {"id":5318639,"name":"Mystical Space Typhoon","type":["魔法","速攻"]}
            ]"#,
        )
        .unwrap();

        assert_eq!(
            cards,
            vec![
                card(
                    89631139,
                    "Blue-Eyes White Dragon",
                    &["怪兽", "龙族", "通常"]
                ),
                card(5318639, "Mystical Space Typhoon", &["魔法", "速攻"]),
            ]
        );
    }

    #[test]
    fn finds_added_cards_by_id() {
        let current_cards = vec![
            card(1, "Existing", &["魔法"]),
            card(2, "New", &["怪兽", "龙族", "通常"]),
        ];
        let previous_cards = vec![card(1, "Old Name", &["陷阱"])];

        assert_eq!(
            find_added_cards(&current_cards, &previous_cards),
            vec![card(2, "New", &["怪兽", "龙族", "通常"])]
        );
    }

    #[test]
    fn extracts_release_tag_from_resolved_github_path() {
        assert_eq!(
            release_tag_from_path("/arshtyi/ygo-cards/releases/tag/0.0.2"),
            Some("0.0.2")
        );
        assert_eq!(
            release_tag_from_path("/arshtyi/ygo-cards/releases/latest"),
            None
        );
    }
}
