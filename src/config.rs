use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

pub(crate) fn read_json_config<T>(relative_path: &str, description: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read {description} {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {description} {}", path.display()))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        card_image_base_url: String,
    }

    #[test]
    fn reads_json_relative_to_manifest() {
        let fixture: Fixture = read_json_config("config/endpoints.json", "test config").unwrap();
        assert!(fixture.card_image_base_url.starts_with("https://"));
    }
}
