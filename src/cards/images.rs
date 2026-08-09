use std::{
    collections::HashMap,
    io::{self, Write},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::{
    Method, StatusCode,
    blocking::{Client, Response},
    header::{CONTENT_TYPE, RANGE},
};

use super::{FailedImageCheck, GenerationOptions, ImageCheckFailure, ImageCheckSummary};
use crate::{
    diagnostics::{self, Diagnostic},
    environment::Environment,
    http::{backoff_delay, is_retryable_status},
};

const MAX_IMAGE_CHECK_ATTEMPTS: u32 = 3;

#[derive(Debug)]
pub(crate) struct ImageResolver {
    mode: ImageMode,
    cache: HashMap<i64, ImageCheck>,
    summary: ImageCheckSummary,
    failures: Vec<ImageCheckFailure>,
    progress: Option<ProgressState>,
}

impl ImageResolver {
    pub(crate) fn new(options: GenerationOptions) -> Result<Self> {
        anyhow::ensure!(
            options.check_images || !options.skip_image_failures,
            "skipping image failures requires image checks"
        );

        let mode = if options.check_images {
            ImageMode::Checking {
                client: Client::builder()
                    .user_agent(concat!(
                        env!("CARGO_PKG_NAME"),
                        "/",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .timeout(Duration::from_secs(20))
                    .build()
                    .context("failed to build HTTP image client")?,
                base_url: crate::endpoints::endpoints()?
                    .card_image_base_url()
                    .to_string(),
            }
        } else {
            ImageMode::UseCardId
        };

        Ok(Self {
            mode,
            cache: HashMap::new(),
            summary: ImageCheckSummary {
                enabled: options.check_images,
                skip_failures: options.skip_image_failures,
                ..ImageCheckSummary::default()
            },
            failures: Vec::new(),
            progress: None,
        })
    }

    pub(crate) fn resolve(
        &mut self,
        environment: Environment,
        id: i64,
        name: &str,
        alias: i64,
    ) -> Option<i64> {
        if matches!(self.mode, ImageMode::UseCardId) {
            return Some(id);
        }

        self.summary.cards_checked += 1;
        let primary = self.check(id);
        if primary.is_found() {
            self.summary.primary_found += 1;
            return Some(id);
        }

        let primary = self.failed_check(id, primary);
        let alias_check = if alias > 0 && alias != id {
            let alias_result = self.check(alias);
            if alias_result.is_found() {
                self.summary.alias_found += 1;
                return Some(alias);
            }
            Some(self.failed_check(alias, alias_result))
        } else {
            None
        };

        self.summary.missing += 1;
        let card_skipped = self.summary.skip_failures;
        diagnostics::record(
            Diagnostic::warning("image.unresolved", "Card image could not be resolved")
                .context("Environment", environment)
                .context("Card ID", id)
                .context("Alias", alias)
                .context("Name", name)
                .context(
                    "Action",
                    if card_skipped {
                        "card skipped"
                    } else {
                        "card kept with image 0"
                    },
                )
                .reason("No primary or distinct alias image candidate passed validation"),
        );
        self.failures.push(ImageCheckFailure {
            environment,
            id,
            name: name.to_string(),
            alias,
            primary,
            alias_check,
            card_skipped,
        });
        if card_skipped {
            self.summary.cards_skipped += 1;
            None
        } else {
            Some(0)
        }
    }

    pub(crate) fn summary(&self) -> ImageCheckSummary {
        self.summary
    }

    pub(crate) fn failures(&self) -> &[ImageCheckFailure] {
        &self.failures
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
        println!();
        self.progress = None;
    }

    fn draw_progress(&self) {
        let Some(progress) = &self.progress else {
            return;
        };

        let width = 30;
        let filled = progress.current.saturating_mul(width) / progress.total.max(1);
        let empty = width - filled;
        print!(
            "\r{} [{}{}] {}/{}",
            progress.label,
            "#".repeat(filled),
            ".".repeat(empty),
            progress.current.min(progress.total),
            progress.total
        );
        let _ = io::stdout().flush();
    }

    fn check(&mut self, id: i64) -> ImageCheck {
        if let Some(result) = self.cache.get(&id) {
            self.summary.cache_hits += 1;
            return result.clone();
        }

        let result = self.image_exists(id);
        match &result {
            ImageCheck::Found => {
                self.summary.unique_urls_found += 1;
            }
            ImageCheck::Missing(_) => {
                self.summary.unique_urls_missing += 1;
            }
            ImageCheck::NetworkError(_) => {
                self.summary.network_errors += 1;
                self.summary.unique_urls_missing += 1;
            }
        }
        if let Some((kind, reason)) = result.failure_details() {
            let url = self.image_url(id).unwrap_or_else(|| {
                diagnostics::record(
                    Diagnostic::error(
                        "internal.image-state",
                        "Image checker entered an inconsistent state",
                    )
                    .context("Image ID", id)
                    .reason("Checking mode has no image URL")
                    .suggestion("Report this as an internal ygo-cards bug"),
                );
                format!("{id}.jpg")
            });
            diagnostics::record(
                Diagnostic::warning("image.candidate-failed", "Image candidate check failed")
                    .context("Image ID", id)
                    .context("Kind", kind)
                    .context("URL", url)
                    .reason(reason),
            );
        }
        self.cache.insert(id, result.clone());
        result
    }

    fn failed_check(&self, id: i64, result: ImageCheck) -> FailedImageCheck {
        let url = self.image_url(id).unwrap_or_else(|| {
            diagnostics::record(
                Diagnostic::error(
                    "internal.image-state",
                    "Image checker entered an inconsistent state",
                )
                .context("Image ID", id)
                .reason("A failed check has no image URL")
                .suggestion("Report this as an internal ygo-cards bug"),
            );
            format!("{id}.jpg")
        });
        let reason = result.failure_reason().unwrap_or_else(|| {
            diagnostics::record(
                Diagnostic::error(
                    "internal.image-state",
                    "Image checker entered an inconsistent state",
                )
                .context("Image ID", id)
                .reason("A successful result was recorded as a failed check")
                .suggestion("Report this as an internal ygo-cards bug"),
            );
            String::from("internal error: missing failure reason")
        });

        FailedImageCheck {
            image_id: id,
            url,
            reason,
        }
    }

    fn image_exists(&self, id: i64) -> ImageCheck {
        let ImageMode::Checking { client, base_url } = &self.mode else {
            return ImageCheck::Found;
        };

        let url = image_url(base_url, id);
        match send_image_request_with_retries(client, Method::HEAD, &url) {
            Ok(response) if response.status() == StatusCode::METHOD_NOT_ALLOWED => self
                .image_exists_with_get(&url)
                .with_context("HEAD returned HTTP 405 Method Not Allowed; GET fallback"),
            Ok(response) => classify_image_response(&response),
            Err(error) => ImageCheck::NetworkError(format!("{error:#}")),
        }
    }

    fn image_exists_with_get(&self, url: &str) -> ImageCheck {
        let ImageMode::Checking { client, .. } = &self.mode else {
            return ImageCheck::Found;
        };

        match send_image_request_with_retries(client, Method::GET, url) {
            Ok(response) => classify_image_response(&response),
            Err(error) => ImageCheck::NetworkError(format!("{error:#}")),
        }
    }

    fn image_url(&self, id: i64) -> Option<String> {
        match &self.mode {
            ImageMode::UseCardId => None,
            ImageMode::Checking { base_url, .. } => Some(image_url(base_url, id)),
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
    Checking { client: Client, base_url: String },
}

#[derive(Debug, Clone)]
enum ImageCheck {
    Found,
    Missing(String),
    NetworkError(String),
}

impl ImageCheck {
    fn is_found(&self) -> bool {
        matches!(self, Self::Found)
    }

    fn failure_reason(self) -> Option<String> {
        match self {
            Self::Found => None,
            Self::Missing(reason) | Self::NetworkError(reason) => Some(reason),
        }
    }

    fn failure_details(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Found => None,
            Self::Missing(reason) => Some(("http-or-content", reason)),
            Self::NetworkError(reason) => Some(("network", reason)),
        }
    }

    fn with_context(self, context: &str) -> Self {
        match self {
            Self::Found => Self::Found,
            Self::Missing(reason) => Self::Missing(format!("{context}: {reason}")),
            Self::NetworkError(reason) => Self::NetworkError(format!("{context}: {reason}")),
        }
    }
}

fn image_url(base_url: &str, id: i64) -> String {
    format!("{}/{id}.jpg", base_url.trim_end_matches('/'))
}

fn classify_image_response(response: &Response) -> ImageCheck {
    if is_image_response(response) {
        ImageCheck::Found
    } else {
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        ImageCheck::Missing(non_image_response_reason(response.status(), content_type))
    }
}

fn non_image_response_reason(status: StatusCode, content_type: Option<&str>) -> String {
    if !status.is_success() {
        return format!("HTTP {status}");
    }

    match content_type {
        Some(content_type) => format!("HTTP {status}; Content-Type {content_type} is not an image"),
        None => format!("HTTP {status}; missing or invalid Content-Type"),
    }
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
            Ok(response) => {
                let delay = backoff_delay(attempt);
                diagnostics::record(
                    Diagnostic::warning("image.request-retry", "Image request will be retried")
                        .context("Method", &method)
                        .context("URL", url)
                        .context(
                            "Attempt",
                            format!("{attempt} of {MAX_IMAGE_CHECK_ATTEMPTS}"),
                        )
                        .context("Retry in", format!("{} seconds", delay.as_secs()))
                        .reason(format!("HTTP {}", response.status())),
                );
                thread::sleep(delay);
            }
            Err(error) if attempt < MAX_IMAGE_CHECK_ATTEMPTS => {
                let delay = backoff_delay(attempt);
                diagnostics::record(
                    Diagnostic::warning("image.request-retry", "Image request will be retried")
                        .context("Method", &method)
                        .context("URL", url)
                        .context(
                            "Attempt",
                            format!("{attempt} of {MAX_IMAGE_CHECK_ATTEMPTS}"),
                        )
                        .context("Retry in", format!("{} seconds", delay.as_secs()))
                        .reason(format!("{:#}", anyhow::Error::new(error))),
                );
                thread::sleep(delay);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to check image with {method} {url} after {MAX_IMAGE_CHECK_ATTEMPTS} attempts"
                    )
                });
            }
        }
    }

    bail!("image request loop ended unexpectedly: method={method} url={url}")
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

    fn options(check_images: bool, skip_image_failures: bool) -> GenerationOptions {
        GenerationOptions {
            check_images,
            skip_image_failures,
        }
    }

    #[test]
    fn builds_image_urls() {
        assert_eq!(
            image_url("https://example.test/cards/", 89631139),
            "https://example.test/cards/89631139.jpg"
        );
    }

    #[test]
    fn describes_non_image_responses() {
        assert_eq!(
            non_image_response_reason(StatusCode::NOT_FOUND, Some("text/html")),
            "HTTP 404 Not Found"
        );
        assert_eq!(
            non_image_response_reason(StatusCode::OK, Some("text/html")),
            "HTTP 200 OK; Content-Type text/html is not an image"
        );
        assert_eq!(
            non_image_response_reason(StatusCode::OK, None),
            "HTTP 200 OK; missing or invalid Content-Type"
        );
    }

    #[test]
    fn keeps_cards_with_failed_images_by_default() {
        let mut resolver = ImageResolver::new(options(true, false)).unwrap();
        resolver
            .cache
            .insert(100, ImageCheck::Missing(String::from("HTTP 404 Not Found")));
        resolver.cache.insert(
            200,
            ImageCheck::NetworkError(String::from("request timed out")),
        );

        assert_eq!(resolver.resolve(Environment::Ot, 100, "Card", 200), Some(0));
        assert_eq!(resolver.summary().missing, 1);
        assert_eq!(resolver.summary().cards_skipped, 0);
        let failure = &resolver.failures()[0];
        assert_eq!(failure.id, 100);
        assert_eq!(failure.alias, 200);
        assert_eq!(failure.primary.image_id, 100);
        assert_eq!(failure.primary.reason, "HTTP 404 Not Found");
        assert_eq!(
            failure.alias_check.as_ref().unwrap().reason,
            "request timed out"
        );
        assert!(!failure.card_skipped);
    }

    #[test]
    fn skips_cards_with_failed_images_when_enabled() {
        let mut resolver = ImageResolver::new(options(true, true)).unwrap();
        resolver
            .cache
            .insert(100, ImageCheck::Missing(String::from("HTTP 404 Not Found")));
        resolver
            .cache
            .insert(200, ImageCheck::Missing(String::from("HTTP 404 Not Found")));

        assert_eq!(resolver.resolve(Environment::Ot, 100, "Card", 200), None);
        assert_eq!(resolver.summary().missing, 1);
        assert_eq!(resolver.summary().cards_skipped, 1);
        assert!(resolver.failures()[0].card_skipped);
    }

    #[test]
    fn uses_an_available_alias_after_a_primary_failure() {
        let mut resolver = ImageResolver::new(options(true, true)).unwrap();
        resolver
            .cache
            .insert(100, ImageCheck::Missing(String::from("HTTP 404 Not Found")));
        resolver.cache.insert(200, ImageCheck::Found);

        assert_eq!(
            resolver.resolve(Environment::Ot, 100, "Card", 200),
            Some(200)
        );
        assert_eq!(resolver.summary().alias_found, 1);
        assert!(resolver.failures().is_empty());
    }

    #[test]
    fn rejects_skipping_without_image_checks() {
        let error = ImageResolver::new(options(false, true)).unwrap_err();

        assert_eq!(
            error.to_string(),
            "skipping image failures requires image checks"
        );
    }
}
