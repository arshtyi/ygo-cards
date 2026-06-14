use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result};
use reqwest::{
    blocking::{Client, Response},
    header::{CONTENT_TYPE, RANGE},
};

const IMAGE_BASE_URL: &str = "https://images.ygoprodeck.com/images/cards_cropped";

#[derive(Debug)]
pub(crate) struct ImageResolver {
    mode: ImageMode,
    cache: HashMap<i64, bool>,
}

impl ImageResolver {
    pub(crate) fn new(check_images: bool) -> Result<Self> {
        let mode = if check_images {
            ImageMode::Checking(
                Client::builder()
                    .user_agent(concat!(
                        env!("CARGO_PKG_NAME"),
                        "/",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .timeout(Duration::from_secs(20))
                    .build()
                    .context("failed to build HTTP image client")?,
            )
        } else {
            ImageMode::UseCardId
        };

        Ok(Self {
            mode,
            cache: HashMap::new(),
        })
    }

    pub(crate) fn resolve(&mut self, id: i64, alias: i64) -> Result<i64> {
        if matches!(self.mode, ImageMode::UseCardId) {
            return Ok(id);
        }

        resolve_image(id, alias, |image_id| self.exists(image_id))
    }

    fn exists(&mut self, id: i64) -> Result<bool> {
        if let Some(exists) = self.cache.get(&id) {
            return Ok(*exists);
        }

        let exists = self.image_exists(id)?;
        self.cache.insert(id, exists);
        Ok(exists)
    }

    fn image_exists(&self, id: i64) -> Result<bool> {
        let ImageMode::Checking(client) = &self.mode else {
            return Ok(true);
        };

        let url = image_url(id);
        let response = client
            .head(&url)
            .send()
            .with_context(|| format!("failed to check image {}", url))?;

        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return self.image_exists_with_get(&url);
        }

        Ok(is_image_response(&response))
    }

    fn image_exists_with_get(&self, url: &str) -> Result<bool> {
        let ImageMode::Checking(client) = &self.mode else {
            return Ok(true);
        };

        let response = client
            .get(url)
            .header(RANGE, "bytes=0-0")
            .send()
            .with_context(|| format!("failed to check image {}", url))?;

        Ok(is_image_response(&response))
    }
}

#[derive(Debug)]
enum ImageMode {
    UseCardId,
    Checking(Client),
}

fn image_url(id: i64) -> String {
    format!("{IMAGE_BASE_URL}/{id}.jpg")
}

fn resolve_image(
    mut id: i64,
    alias: i64,
    mut exists: impl FnMut(i64) -> Result<bool>,
) -> Result<i64> {
    if exists(id)? {
        return Ok(id);
    }

    if alias > 0 && exists(alias)? {
        id = alias;
    } else {
        id = 0;
    }

    Ok(id)
}

fn is_image_response(response: &Response) -> bool {
    response.status().is_success()
        && response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|content_type| content_type.starts_with("image/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_image_urls() {
        assert_eq!(
            image_url(89631139),
            "https://images.ygoprodeck.com/images/cards_cropped/89631139.jpg"
        );
    }

    #[test]
    fn resolves_image_id_with_alias_fallback() {
        assert_eq!(resolve_image(100, 200, |id| Ok(id == 100)).unwrap(), 100);
        assert_eq!(resolve_image(100, 200, |id| Ok(id == 200)).unwrap(), 200);
        assert_eq!(resolve_image(100, 0, |_| Ok(false)).unwrap(), 0);
        assert_eq!(resolve_image(100, 200, |_| Ok(false)).unwrap(), 0);
    }
}
