use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

const URLS_CONFIG: &str = "config/urls.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlConfig {
    resources: ResourceUrls,
    latest_json: LatestJsonUrls,
    image_base_url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResourceUrls {
    ot_cards_database: String,
    ot_forbidden_list: String,
    rd_cards_database: String,
    rd_forbidden_list: String,
}

#[derive(Debug, Deserialize)]
struct LatestJsonUrls {
    ot: String,
    rd: String,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResourceUrlKey {
    OtCardsDatabase,
    OtForbiddenList,
    RdCardsDatabase,
    RdForbiddenList,
}

pub fn urls() -> Result<&'static UrlConfig> {
    static URLS: OnceLock<std::result::Result<UrlConfig, String>> = OnceLock::new();
    URLS.get_or_init(|| load_urls().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow!("failed to load URL config: {error}"))
}

impl UrlConfig {
    pub(crate) fn resource_url(&self, key: ResourceUrlKey) -> &str {
        match key {
            ResourceUrlKey::OtCardsDatabase => &self.resources.ot_cards_database,
            ResourceUrlKey::OtForbiddenList => &self.resources.ot_forbidden_list,
            ResourceUrlKey::RdCardsDatabase => &self.resources.rd_cards_database,
            ResourceUrlKey::RdForbiddenList => &self.resources.rd_forbidden_list,
        }
    }

    pub fn latest_json_url(&self, label: &str) -> Result<&str> {
        match label {
            "OT" => Ok(&self.latest_json.ot),
            "RD" => Ok(&self.latest_json.rd),
            _ => bail!("unsupported environment label {label}"),
        }
    }

    pub fn image_base_url(&self) -> &str {
        &self.image_base_url
    }

    fn validate(&self) -> Result<()> {
        validate_url(
            "resources.otCardsDatabase",
            &self.resources.ot_cards_database,
        )?;
        validate_url(
            "resources.otForbiddenList",
            &self.resources.ot_forbidden_list,
        )?;
        validate_url(
            "resources.rdCardsDatabase",
            &self.resources.rd_cards_database,
        )?;
        validate_url(
            "resources.rdForbiddenList",
            &self.resources.rd_forbidden_list,
        )?;
        validate_url("latestJson.ot", &self.latest_json.ot)?;
        validate_url("latestJson.rd", &self.latest_json.rd)?;
        validate_url("imageBaseUrl", &self.image_base_url)?;
        Ok(())
    }
}

fn load_urls() -> Result<UrlConfig> {
    let config: UrlConfig = read_config(URLS_CONFIG)?;
    config.validate()?;
    Ok(config)
}

fn read_config<T>(path: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let path = manifest_path(path);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read URL config {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse URL config {}", path.display()))
}

fn validate_url(field: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("URL config field {field} must not be empty");
    }
    if trimmed != value {
        bail!("URL config field {field} must not contain leading or trailing whitespace");
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        bail!("URL config field {field} must use http or https: {trimmed}");
    }
    Ok(())
}

fn manifest_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_url_config() {
        let config = urls().unwrap();

        assert!(
            config
                .resource_url(ResourceUrlKey::OtCardsDatabase)
                .ends_with("/cards.cdb")
        );
        assert!(config.latest_json_url("RD").unwrap().ends_with("/rd.json"));
        assert!(config.image_base_url().starts_with("https://"));
    }

    #[test]
    fn rejects_invalid_url_values() {
        assert!(validate_url("field", "").is_err());
        assert!(validate_url("field", " https://example.test").is_err());
        assert!(validate_url("field", "file:///tmp/cards.cdb").is_err());
        assert!(validate_url("field", "https://example.test/cards.cdb").is_ok());
    }
}
