use std::{
    collections::HashMap,
    io::{self, Write},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use reqwest::{
    Method, StatusCode,
    blocking::{Client, Response},
    header::{CONTENT_TYPE, RANGE},
};

use super::ImageSummary;

const IMAGE_BASE_URL: &str = "https://images.ygoprodeck.com/images/cards_cropped";
const MAX_IMAGE_CHECK_ATTEMPTS: u32 = 3;

#[derive(Debug)]
pub(crate) struct ImageResolver {
    mode: ImageMode,
    cache: HashMap<i64, bool>,
    summary: ImageSummary,
    progress: Option<ProgressState>,
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
            summary: ImageSummary {
                enabled: check_images,
                ..ImageSummary::default()
            },
            progress: None,
        })
    }

    pub(crate) fn resolve(&mut self, id: i64, alias: i64) -> Result<i64> {
        if matches!(self.mode, ImageMode::UseCardId) {
            return Ok(id);
        }

        self.summary.cards_checked += 1;
        let image = resolve_image(id, alias, |image_id| self.exists(image_id));
        if image == id {
            self.summary.primary_found += 1;
        } else if alias > 0 && image == alias {
            self.summary.alias_found += 1;
        } else {
            self.summary.missing += 1;
        }

        Ok(image)
    }

    pub(crate) fn summary(&self) -> ImageSummary {
        self.summary
    }

    pub(crate) fn start_progress(&mut self, total: usize, label: &'static str) {
        if matches!(self.mode, ImageMode::UseCardId) || total == 0 {
            return;
        }

        self.progress = Some(ProgressState::new(total, label));
        self.draw_progress();
    }

    pub(crate) fn advance_progress(&mut self) {
        let Some(progress) = &mut self.progress else {
            return;
        };
        progress.current += 1;
        if progress.should_draw() {
            self.draw_progress();
        }
    }

    pub(crate) fn finish_progress(&mut self) {
        let Some(progress) = &mut self.progress else {
            return;
        };
        progress.current = progress.total;
        self.draw_progress();
        eprintln!();
        self.progress = None;
    }

    fn draw_progress(&self) {
        let Some(progress) = &self.progress else {
            return;
        };

        let width = 30;
        let filled = progress.current.saturating_mul(width) / progress.total.max(1);
        let empty = width - filled;
        eprint!(
            "\r{} [{}{}] {}/{}",
            progress.label,
            "#".repeat(filled),
            ".".repeat(empty),
            progress.current.min(progress.total),
            progress.total
        );
        let _ = io::stderr().flush();
    }

    fn exists(&mut self, id: i64) -> bool {
        if let Some(exists) = self.cache.get(&id) {
            self.summary.cache_hits += 1;
            return *exists;
        }

        let exists = match self.image_exists(id) {
            ImageCheck::Found => {
                self.summary.unique_urls_found += 1;
                true
            }
            ImageCheck::Missing => {
                self.summary.unique_urls_missing += 1;
                false
            }
            ImageCheck::NetworkError(error) => {
                self.summary.network_errors += 1;
                self.summary.unique_urls_missing += 1;
                eprintln!("image check failed for {}: {error}", image_url(id));
                false
            }
        };
        self.cache.insert(id, exists);
        exists
    }

    fn image_exists(&self, id: i64) -> ImageCheck {
        let ImageMode::Checking(client) = &self.mode else {
            return ImageCheck::Found;
        };

        let url = image_url(id);
        match send_image_request_with_retries(client, Method::HEAD, &url) {
            Ok(response) if response.status() == StatusCode::METHOD_NOT_ALLOWED => {
                self.image_exists_with_get(&url)
            }
            Ok(response) => {
                if is_image_response(&response) {
                    ImageCheck::Found
                } else {
                    ImageCheck::Missing
                }
            }
            Err(error) => ImageCheck::NetworkError(error.to_string()),
        }
    }

    fn image_exists_with_get(&self, url: &str) -> ImageCheck {
        let ImageMode::Checking(client) = &self.mode else {
            return ImageCheck::Found;
        };

        match send_image_request_with_retries(client, Method::GET, url) {
            Ok(response) if is_image_response(&response) => ImageCheck::Found,
            Ok(_) => ImageCheck::Missing,
            Err(error) => ImageCheck::NetworkError(error.to_string()),
        }
    }
}

#[derive(Debug)]
struct ProgressState {
    label: &'static str,
    total: usize,
    current: usize,
    last_drawn_percent: usize,
}

impl ProgressState {
    fn new(total: usize, label: &'static str) -> Self {
        Self {
            label,
            total,
            current: 0,
            last_drawn_percent: usize::MAX,
        }
    }

    fn should_draw(&mut self) -> bool {
        let percent = self.current.saturating_mul(100) / self.total.max(1);
        if percent != self.last_drawn_percent || self.current >= self.total {
            self.last_drawn_percent = percent;
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
enum ImageMode {
    UseCardId,
    Checking(Client),
}

#[derive(Debug)]
enum ImageCheck {
    Found,
    Missing,
    NetworkError(String),
}

fn image_url(id: i64) -> String {
    format!("{IMAGE_BASE_URL}/{id}.jpg")
}

fn resolve_image(mut id: i64, alias: i64, mut exists: impl FnMut(i64) -> bool) -> i64 {
    if exists(id) {
        return id;
    }

    if alias > 0 && exists(alias) {
        id = alias;
    } else {
        id = 0;
    }

    id
}

fn send_image_request_with_retries(client: &Client, method: Method, url: &str) -> Result<Response> {
    for attempt in 1..=MAX_IMAGE_CHECK_ATTEMPTS {
        let request = client.request(method.clone(), url);
        let request = if method == Method::GET {
            request.header(RANGE, "bytes=0-0")
        } else {
            request
        };

        match request.send() {
            Ok(response) if !is_retryable_status(response.status()) => return Ok(response),
            Ok(response) if attempt == MAX_IMAGE_CHECK_ATTEMPTS => return Ok(response),
            Ok(_) | Err(_) if attempt < MAX_IMAGE_CHECK_ATTEMPTS => {
                thread::sleep(backoff_delay(attempt));
            }
            Ok(response) => return Ok(response),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to check image {}", url));
            }
        }
    }

    unreachable!("image check attempts are bounded by MAX_IMAGE_CHECK_ATTEMPTS")
}

fn is_image_response(response: &Response) -> bool {
    response.status().is_success()
        && response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|content_type| content_type.starts_with("image/"))
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_secs(u64::from(attempt * attempt))
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
        assert_eq!(resolve_image(100, 200, |id| id == 100), 100);
        assert_eq!(resolve_image(100, 200, |id| id == 200), 200);
        assert_eq!(resolve_image(100, 0, |_| false), 0);
        assert_eq!(resolve_image(100, 200, |_| false), 0);
    }
}
