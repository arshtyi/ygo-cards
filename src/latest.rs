use std::{collections::HashSet, fs, path::Path, time::Duration};

use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::Deserialize;
use ygo_cards::cards::WriteReport;

pub(crate) fn compare_latest_release(
    reports: &[&WriteReport],
) -> Result<Vec<LatestComparisonReport>> {
    let url_config = ygo_cards::urls::urls()?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build latest release HTTP client")?;

    reports
        .iter()
        .map(|report| compare_latest_release_file(&client, url_config, report))
        .collect()
}

fn compare_latest_release_file(
    client: &reqwest::blocking::Client,
    url_config: &ygo_cards::urls::UrlConfig,
    report: &WriteReport,
) -> Result<LatestComparisonReport> {
    let latest_url = url_config.latest_json_url(report.label)?.to_string();
    let current_cards = read_card_summaries(&report.path)
        .with_context(|| format!("failed to read current {} cards", report.label))?;

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
            ygo_cards::diagnostics::warning(format_args!(
                "latest release comparison skipped: environment={} url={} reason=HTTP 404 Not Found",
                report.label, latest_url
            ));
            (LatestComparisonStatus::NotFound, Vec::new())
        }
        LatestCardsFetch::Unavailable(reason) => {
            ygo_cards::diagnostics::warning(format_args!(
                "latest release comparison skipped: environment={} url={} reason={reason}",
                report.label, latest_url
            ));
            (LatestComparisonStatus::Unavailable(reason), Vec::new())
        }
    };

    Ok(LatestComparisonReport {
        label: report.label,
        latest_url,
        current_cards: current_cards.len(),
        status,
        added_cards,
    })
}

fn fetch_latest_card_summaries(client: &reqwest::blocking::Client, url: &str) -> LatestCardsFetch {
    let response = match client.get(url).send() {
        Ok(response) => response,
        Err(error) => return LatestCardsFetch::Unavailable(format!("download failed: {error}")),
    };

    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return LatestCardsFetch::NotFound,
        status => return LatestCardsFetch::Unavailable(format!("HTTP {status}")),
    }

    let text = match response.text() {
        Ok(text) => text,
        Err(error) => return LatestCardsFetch::Unavailable(format!("read failed: {error}")),
    };

    match parse_card_summaries(&text) {
        Ok(cards) => LatestCardsFetch::Cards(cards),
        Err(error) => LatestCardsFetch::Unavailable(format!("invalid JSON: {error}")),
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
pub(crate) struct LatestComparisonReport {
    pub(crate) label: &'static str,
    pub(crate) latest_url: String,
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
}
