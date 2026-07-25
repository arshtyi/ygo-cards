use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::RETRY_AFTER,
};

use crate::{
    diagnostics,
    http::{backoff_delay, is_retryable_status},
    urls::{self, ResourceUrlKey, UrlConfig},
};

const ASSETS_DIR: &str = "assets";
const MAX_DOWNLOAD_ATTEMPTS: u32 = 4;
const RATE_LIMIT_DELAY: Duration = Duration::from_secs(60);

const RESOURCE_DEFINITIONS: &[ResourceDefinition] = &[
    ResourceDefinition {
        name: "ot cards database",
        url: ResourceUrlKey::OtCardsDatabase,
        path: &["ot", "cards.cdb"],
    },
    ResourceDefinition {
        name: "ot forbidden list",
        url: ResourceUrlKey::OtForbiddenList,
        path: &["ot", "lflist.conf"],
    },
    ResourceDefinition {
        name: "rd cards database",
        url: ResourceUrlKey::RdCardsDatabase,
        path: &["rd", "rd_standard.cdb"],
    },
    ResourceDefinition {
        name: "rd forbidden list",
        url: ResourceUrlKey::RdForbiddenList,
        path: &["rd", "lflist.conf"],
    },
];

#[derive(Debug)]
struct ResourceDefinition {
    name: &'static str,
    url: ResourceUrlKey,
    path: &'static [&'static str],
}

impl ResourceDefinition {
    fn to_resource<'a>(&'a self, config: &'a UrlConfig) -> Resource<'a> {
        Resource {
            name: self.name,
            url: config.resource_url(self.url),
            path: self.path,
        }
    }
}

#[derive(Debug)]
struct Resource<'a> {
    name: &'static str,
    url: &'a str,
    path: &'static [&'static str],
}

#[derive(Debug)]
pub struct DownloadedResource {
    pub path: PathBuf,
    pub bytes: u64,
    pub attempts: u32,
}

pub fn download_all() -> Result<Vec<DownloadedResource>> {
    let url_config = urls::urls()?;
    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build resource download HTTP client")?;

    RESOURCE_DEFINITIONS
        .iter()
        .map(|definition| {
            let resource = definition.to_resource(url_config);
            download_resource(&client, &resource)
        })
        .collect()
}

pub fn ensure_all() -> Result<()> {
    let url_config = urls::urls()?;
    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build resource download HTTP client")?;

    for definition in RESOURCE_DEFINITIONS {
        let resource = definition.to_resource(url_config);
        if validate_asset(&asset_path(&resource)).is_ok() {
            continue;
        }

        download_resource(&client, &resource)?;
    }

    Ok(())
}

fn download_resource(client: &Client, resource: &Resource<'_>) -> Result<DownloadedResource> {
    let path = asset_path(resource);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create asset directory {}", parent.display()))?;
    }

    let temp_path = temp_path_for(&path)?;
    let mut last_error = None;

    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        if let Err(error) = remove_file_if_exists(&temp_path) {
            diagnostics::warning(format_args!(
                "failed to remove stale temporary asset: path={} reason={error}; continuing",
                temp_path.display()
            ));
        }
        match download_resource_once(client, resource, &path, &temp_path) {
            Ok(bytes) => {
                return Ok(DownloadedResource {
                    path,
                    bytes,
                    attempts: attempt,
                });
            }
            Err(error) if attempt < MAX_DOWNLOAD_ATTEMPTS => {
                let delay = backoff_delay(attempt);
                diagnostics::warning(format_args!(
                    "resource download attempt failed: resource={:?} url={} destination={} attempt={}/{} reason={error:#}; retry_in={}s",
                    resource.name,
                    resource.url,
                    path.display(),
                    attempt,
                    MAX_DOWNLOAD_ATTEMPTS,
                    delay.as_secs()
                ));
                last_error = Some(error);
                thread::sleep(delay);
            }
            Err(error) => last_error = Some(error),
        }
    }

    let last_error = last_error.context(format!(
        "resource download loop ended without an error: resource={:?} url={}",
        resource.name, resource.url
    ))?;
    Err(last_error).with_context(|| {
        format!(
            "failed to download {} from {} to {}",
            resource.name,
            resource.url,
            path.display()
        )
    })
}

fn download_resource_once(
    client: &Client,
    resource: &Resource<'_>,
    path: &Path,
    temp_path: &Path,
) -> Result<u64> {
    let mut response = send_with_retries(client, resource)?;
    let expected_length = response.content_length();
    let bytes = write_response(&mut response, temp_path)
        .with_context(|| format!("failed to write temporary asset {}", temp_path.display()))?;
    validate_download_size(resource, bytes, expected_length)?;
    replace_file(temp_path, path).with_context(|| {
        format!(
            "failed to move temporary asset {} into place at {}",
            temp_path.display(),
            path.display()
        )
    })?;

    Ok(bytes)
}

fn send_with_retries(client: &Client, resource: &Resource<'_>) -> Result<Response> {
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        match client.get(resource.url).send() {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let status = response.status();
                if !is_retryable_status(status) || attempt == MAX_DOWNLOAD_ATTEMPTS {
                    bail!(
                        "failed to download {} from {}: HTTP {}",
                        resource.name,
                        resource.url,
                        status
                    );
                }

                let delay = retry_delay(&response, attempt);
                diagnostics::warning(format_args!(
                    "resource request will be retried: resource={:?} url={} attempt={}/{} status=\"HTTP {}\" retry_in={}s",
                    resource.name,
                    resource.url,
                    attempt,
                    MAX_DOWNLOAD_ATTEMPTS,
                    status,
                    delay.as_secs()
                ));
                thread::sleep(delay);
            }
            Err(error) => {
                if attempt == MAX_DOWNLOAD_ATTEMPTS {
                    return Err(error).with_context(|| {
                        format!(
                            "failed to download {} from {} after {} attempts",
                            resource.name, resource.url, MAX_DOWNLOAD_ATTEMPTS
                        )
                    });
                }

                let delay = backoff_delay(attempt);
                diagnostics::warning(format_args!(
                    "resource request will be retried: resource={:?} url={} attempt={}/{} reason={error}; retry_in={}s",
                    resource.name,
                    resource.url,
                    attempt,
                    MAX_DOWNLOAD_ATTEMPTS,
                    delay.as_secs()
                ));
                thread::sleep(delay);
            }
        }
    }

    bail!(
        "resource request loop ended unexpectedly for {} from {}",
        resource.name,
        resource.url
    )
}

fn retry_delay(response: &Response, attempt: u32) -> Duration {
    let delay = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or_else(|| backoff_delay(attempt));

    if response.status() == StatusCode::TOO_MANY_REQUESTS {
        delay.max(RATE_LIMIT_DELAY)
    } else {
        delay
    }
}

fn asset_path(resource: &Resource<'_>) -> PathBuf {
    asset_path_for_parts(resource.path)
}

fn asset_path_for_parts(path_parts: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(ASSETS_DIR);
    path.extend(path_parts);
    path
}

fn validate_asset(path: &Path) -> Result<()> {
    let metadata =
        fs::metadata(path).with_context(|| format!("asset {} is missing", path.display()))?;
    if metadata.len() == 0 {
        bail!("asset {} is empty", path.display());
    }

    Ok(())
}

fn validate_download_size(
    resource: &Resource,
    bytes: u64,
    expected_length: Option<u64>,
) -> Result<()> {
    if bytes == 0 {
        bail!("downloaded empty asset for {}", resource.name);
    }

    if let Some(expected_length) = expected_length {
        if bytes != expected_length {
            bail!(
                "downloaded {} bytes for {}, expected {}",
                bytes,
                resource.name,
                expected_length
            );
        }
    }

    Ok(())
}

fn temp_path_for(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .context("asset path must have a file name")?
        .to_string_lossy();

    Ok(path.with_file_name(format!("{file_name}.part")))
}

fn write_response(response: &mut impl Read, path: &Path) -> io::Result<u64> {
    let mut file = fs::File::create(path)?;
    let bytes = io::copy(response, &mut file)?;
    file.sync_all()?;
    Ok(bytes)
}

fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    let backup = backup_path_for(to)?;
    let had_existing = to.exists();

    if had_existing {
        remove_file_if_exists(&backup)?;
        fs::rename(to, &backup)?;
    }

    match fs::rename(from, to) {
        Ok(()) => {
            if had_existing {
                if let Err(error) = remove_file_if_exists(&backup) {
                    diagnostics::warning(format_args!(
                        "failed to remove replaced asset backup: path={} reason={error}",
                        backup.display()
                    ));
                }
            }
            Ok(())
        }
        Err(error) => {
            if had_existing {
                if let Err(restore_error) = fs::rename(&backup, to) {
                    return Err(io::Error::new(
                        error.kind(),
                        format!(
                            "failed to install {}: {error}; also failed to restore backup {}: {restore_error}",
                            to.display(),
                            backup.display()
                        ),
                    ));
                }
            }
            Err(error)
        }
    }
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn backup_path_for(path: &Path) -> io::Result<PathBuf> {
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{file_name}.bak")))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn asset_paths_are_unique_and_scoped() {
        let mut paths = HashSet::new();

        for definition in RESOURCE_DEFINITIONS {
            let path = asset_path_for_parts(definition.path);
            assert!(path.starts_with(ASSETS_DIR));
            assert!(paths.insert(path));
        }
    }
}
