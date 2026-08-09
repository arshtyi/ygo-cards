use std::sync::OnceLock;

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;

use crate::{config::read_json_config, environment::Environment};

const ENDPOINTS_PATH: &str = "config/endpoints.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Endpoints {
    source_resources: SourceResourceUrls,
    published_datasets: PublishedDatasetUrls,
    card_image_base_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SourceResourceUrls {
    ot_cards_database: String,
    ot_forbidden_list: String,
    rd_cards_database: String,
    rd_forbidden_list: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishedDatasetUrls {
    ot: String,
    rd: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SourceResource {
    OtCardsDatabase,
    OtForbiddenList,
    RdCardsDatabase,
    RdForbiddenList,
}

pub(crate) fn endpoints() -> Result<&'static Endpoints> {
    static ENDPOINTS: OnceLock<std::result::Result<Endpoints, String>> = OnceLock::new();
    ENDPOINTS
        .get_or_init(|| load_endpoints().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow!("failed to load endpoint config: {error}"))
}

impl Endpoints {
    pub(crate) fn source_url(&self, resource: SourceResource) -> &str {
        match resource {
            SourceResource::OtCardsDatabase => &self.source_resources.ot_cards_database,
            SourceResource::OtForbiddenList => &self.source_resources.ot_forbidden_list,
            SourceResource::RdCardsDatabase => &self.source_resources.rd_cards_database,
            SourceResource::RdForbiddenList => &self.source_resources.rd_forbidden_list,
        }
    }

    pub(crate) fn published_dataset_url(&self, environment: Environment) -> &str {
        match environment {
            Environment::Ot => &self.published_datasets.ot,
            Environment::Rd => &self.published_datasets.rd,
        }
    }

    pub(crate) fn card_image_base_url(&self) -> &str {
        &self.card_image_base_url
    }

    fn validate(&self) -> Result<()> {
        validate_url(
            "sourceResources.otCardsDatabase",
            &self.source_resources.ot_cards_database,
        )?;
        validate_url(
            "sourceResources.otForbiddenList",
            &self.source_resources.ot_forbidden_list,
        )?;
        validate_url(
            "sourceResources.rdCardsDatabase",
            &self.source_resources.rd_cards_database,
        )?;
        validate_url(
            "sourceResources.rdForbiddenList",
            &self.source_resources.rd_forbidden_list,
        )?;
        validate_url("publishedDatasets.ot", &self.published_datasets.ot)?;
        validate_url("publishedDatasets.rd", &self.published_datasets.rd)?;
        validate_url("cardImageBaseUrl", &self.card_image_base_url)?;
        Ok(())
    }
}

fn load_endpoints() -> Result<Endpoints> {
    let config: Endpoints = read_json_config(ENDPOINTS_PATH, "endpoint config")?;
    config.validate()?;
    Ok(config)
}

fn validate_url(field: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("endpoint config field {field} must not be empty");
    }
    if trimmed != value {
        bail!("endpoint config field {field} must not contain leading or trailing whitespace");
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        bail!("endpoint config field {field} must use http or https: {trimmed}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_endpoint_config() {
        let config = endpoints().unwrap();

        assert!(
            config
                .source_url(SourceResource::OtCardsDatabase)
                .ends_with("/cards.cdb")
        );
        assert!(
            config
                .published_dataset_url(Environment::Rd)
                .ends_with("/rd.json")
        );
        assert!(config.card_image_base_url().starts_with("https://"));
    }

    #[test]
    fn rejects_invalid_url_values() {
        assert!(validate_url("field", "").is_err());
        assert!(validate_url("field", " https://example.test").is_err());
        assert!(validate_url("field", "file:///tmp/cards.cdb").is_err());
        assert!(validate_url("field", "https://example.test/cards.cdb").is_ok());
    }
}
