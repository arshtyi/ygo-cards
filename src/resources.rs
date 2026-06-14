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

const ASSETS_DIR: &str = "assets";
const MAX_DOWNLOAD_ATTEMPTS: u32 = 4;
const RATE_LIMIT_DELAY: Duration = Duration::from_secs(60);

const RESOURCES: &[Resource] = &[
    Resource {
        name: "ot cards database",
        url: "https://raw.githubusercontent.com/purerosefallen/ygopro/master/cards.cdb",
        path: &["ot", "cards.cdb"],
    },
    Resource {
        name: "ot forbidden list",
        url: "https://raw.githubusercontent.com/purerosefallen/ygopro/master/lflist.conf",
        path: &["ot", "lflist.conf"],
    },
    Resource {
        name: "ot field strings",
        url: "https://raw.githubusercontent.com/purerosefallen/ygopro/master/strings.conf",
        path: &["ot", "strings.conf"],
    },
    Resource {
        name: "rd cards database",
        url: "https://code.moenext.com/mycard/ygopro-rush-duel/-/raw/master/RD%20Standard.cdb",
        path: &["rd", "rd_standard.cdb"],
    },
    Resource {
        name: "rd forbidden list",
        url: "https://code.moenext.com/mycard/ygopro-rush-duel/-/raw/master/lflist.conf",
        path: &["rd", "lflist.conf"],
    },
];

#[derive(Debug)]
struct Resource {
    name: &'static str,
    url: &'static str,
    path: &'static [&'static str],
}

#[derive(Debug)]
pub struct DownloadedResource {
    pub path: PathBuf,
    pub bytes: u64,
}

pub fn download_all() -> Result<Vec<DownloadedResource>> {
    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client")?;

    RESOURCES
        .iter()
        .map(|resource| download_resource(&client, resource))
        .collect()
}

pub fn ensure_all() -> Result<()> {
    let client = Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client")?;

    for resource in RESOURCES {
        if asset_path(resource).exists() {
            continue;
        }

        download_resource(&client, resource)?;
    }

    Ok(())
}

fn download_resource(client: &Client, resource: &Resource) -> Result<DownloadedResource> {
    let path = asset_path(resource);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create asset directory {}", parent.display()))?;
    }

    let mut response = send_with_retries(client, resource)?;

    let temp_path = temp_path_for(&path)?;
    let bytes = write_response(&mut response, &temp_path)
        .with_context(|| format!("failed to write temporary asset {}", temp_path.display()))?;
    replace_file(&temp_path, &path)
        .with_context(|| format!("failed to move asset into place {}", path.display()))?;

    Ok(DownloadedResource { path, bytes })
}

fn send_with_retries(client: &Client, resource: &Resource) -> Result<Response> {
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

                thread::sleep(retry_delay(&response, attempt));
            }
            Err(error) => {
                if attempt == MAX_DOWNLOAD_ATTEMPTS {
                    return Err(error)
                        .with_context(|| format!("failed to download {}", resource.name));
                }

                thread::sleep(backoff_delay(attempt));
            }
        }
    }

    unreachable!("download attempts are bounded by MAX_DOWNLOAD_ATTEMPTS")
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

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_secs(u64::from(attempt * attempt))
}

fn asset_path(resource: &Resource) -> PathBuf {
    let mut path = PathBuf::from(ASSETS_DIR);
    path.extend(resource.path);
    path
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
    if to.exists() {
        fs::remove_file(to)?;
    }

    fs::rename(from, to)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn asset_paths_are_unique_and_scoped() {
        let mut paths = HashSet::new();

        for resource in RESOURCES {
            let path = asset_path(resource);
            assert!(path.starts_with(ASSETS_DIR));
            assert!(paths.insert(path));
        }
    }
}
