use std::{
    collections::{BTreeSet, HashMap},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use crate::http::{backoff_delay, is_retryable_status};

const MAX_SEARCH_ATTEMPTS: u32 = 3;
const NAME_FIELDS: &[&str] = &[
    "cn_name", "sc_name", "md_name", "nwbbs_n", "cnocg_n", "jp_name", "en_name", "wiki_en",
];

pub(super) struct CardSearch {
    client: Client,
    url: reqwest::Url,
}

impl CardSearch {
    pub(super) fn new(url: &str) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent(concat!(
                    env!("CARGO_PKG_NAME"),
                    "/",
                    env!("CARGO_PKG_VERSION")
                ))
                .timeout(Duration::from_secs(30))
                .build()
                .context("failed to build Genesys card search HTTP client")?,
            url: reqwest::Url::parse(url).context("invalid Genesys card search URL")?,
        })
    }

    pub(super) fn resolve(&self, name: &str) -> Result<i64> {
        let mut url = self.url.clone();
        url.query_pairs_mut().append_pair("search", name);
        for attempt in 1..=MAX_SEARCH_ATTEMPTS {
            let response = self.client.get(url.clone()).send();
            match response {
                Ok(response) => {
                    if is_retryable_status(response.status()) && attempt < MAX_SEARCH_ATTEMPTS {
                        thread::sleep(backoff_delay(attempt));
                        continue;
                    }
                    let text = response
                        .error_for_status()
                        .context("card search returned an unsuccessful HTTP status")?
                        .text()
                        .context("failed to read card search response")?;
                    return resolve_response(&text, name);
                }
                Err(_) if attempt < MAX_SEARCH_ATTEMPTS => thread::sleep(backoff_delay(attempt)),
                Err(error) => return Err(error).context("Genesys card search request failed"),
            }
        }
        bail!("Genesys card search exhausted its retries")
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    result: Vec<SearchCard>,
}

#[derive(Deserialize)]
struct SearchCard {
    id: i64,
    #[serde(flatten)]
    fields: HashMap<String, Value>,
}

fn resolve_response(text: &str, name: &str) -> Result<i64> {
    let response: SearchResponse =
        serde_json::from_str(text).context("failed to parse Genesys card search response")?;
    let mut ids = BTreeSet::new();
    let mut exact_ids = BTreeSet::new();
    for card in response.result {
        ensure!(
            card.id > 0,
            "card search returned an invalid ID: {}",
            card.id
        );
        ids.insert(card.id);
        if NAME_FIELDS.iter().any(|field| {
            card.fields
                .get(*field)
                .and_then(Value::as_str)
                .is_some_and(|candidate| candidate.trim().eq_ignore_ascii_case(name))
        }) {
            exact_ids.insert(card.id);
        }
    }
    // A sole fuzzy result also resolves upstream spelling differences (O7 / Ω7).
    let matches = if exact_ids.is_empty() { ids } else { exact_ids };
    ensure!(
        !matches.is_empty(),
        "card search returned no matches for {name:?}"
    );
    ensure!(
        matches.len() == 1,
        "ambiguous card search for {name:?}: IDs {matches:?}"
    );
    matches
        .into_iter()
        .next()
        .context("missing resolved card ID")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_unique_fuzzy_match_for_upstream_spelling() {
        let json = r#"{"result":[{"id":6195332,"en_name":"Exstellarknight Constellar Ptolemy Ω7"}],"next":0}"#;
        assert_eq!(
            resolve_response(json, "Exstellarknight Constellar Ptolemy O7").unwrap(),
            6195332
        );
    }

    #[test]
    fn prefers_exact_name_over_related_search_results() {
        let json = r#"{"result":[{"id":1,"en_name":"Related card"},{"id":2,"en_name":"Card"},{"id":2,"en_name":"Card"}]}"#;
        assert_eq!(resolve_response(json, "card").unwrap(), 2);
        let json = r#"{"result":[{"id":1,"cn_name":"卡"},{"id":2,"cn_name":"卡片"}]}"#;
        assert_eq!(resolve_response(json, "卡").unwrap(), 1);
    }

    #[test]
    fn rejects_missing_invalid_and_ambiguous_search_results() {
        for json in [
            "not json",
            "{}",
            r#"{"result":[]}"#,
            r#"{"result":[{"id":0}]}"#,
            r#"{"result":[{"id":-1}]}"#,
            r#"{"result":[{"id":"1"}]}"#,
            r#"{"result":[{"id":1},{"id":2}]}"#,
            r#"{"result":[{"id":1,"en_name":"Card"},{"id":2,"en_name":"Card"}]}"#,
        ] {
            assert!(resolve_response(json, "Card").is_err(), "{json}");
        }
    }
}
