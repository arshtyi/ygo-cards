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
    diagnostics::{self, Diagnostic},
    endpoints::{self, Endpoints, SourceResource},
    http::{backoff_delay, is_retryable_status},
};

const ASSETS_DIR: &str = "assets";
const MAX_DOWNLOAD_ATTEMPTS: u32 = 4;
const RATE_LIMIT_DELAY: Duration = Duration::from_secs(60);

const RESOURCE_DEFINITIONS: &[ResourceDefinition] = &[
    ResourceDefinition {
        name: "ot cards database",
        source: SourceResource::OtCardsDatabase,
        path: &["ot", "cards.cdb"],
    },
    ResourceDefinition {
        name: "ot forbidden list",
        source: SourceResource::OtForbiddenList,
        path: &["ot", "lflist.conf"],
    },
    ResourceDefinition {
        name: "ot genesys list",
        source: SourceResource::OtGenesysList,
        path: &["ot", "genesys.conf"],
    },
    ResourceDefinition {
        name: "rd cards database",
        source: SourceResource::RdCardsDatabase,
        path: &["rd", "rd_standard.cdb"],
    },
    ResourceDefinition {
        name: "rd forbidden list",
        source: SourceResource::RdForbiddenList,
        path: &["rd", "lflist.conf"],
    },
];

#[derive(Debug)]
struct ResourceDefinition {
    name: &'static str,
    source: SourceResource,
    path: &'static [&'static str],
}

impl ResourceDefinition {
    fn resolve<'a>(&'a self, endpoints: &'a Endpoints) -> Resource<'a> {
        Resource {
            name: self.name,
            url: endpoints.source_url(self.source),
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

pub(crate) fn prepare(refresh: bool) -> Result<()> {
    let endpoints = endpoints::endpoints()?;
    let client = resource_client()?;

    for definition in RESOURCE_DEFINITIONS {
        let resource = definition.resolve(endpoints);
        if !refresh && validate_asset(&asset_path(&resource)).is_ok() {
            continue;
        }

        download_resource(&client, &resource)?;
    }

    Ok(())
}

fn resource_client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build resource download HTTP client")
}

fn download_resource(client: &Client, resource: &Resource<'_>) -> Result<()> {
    let path = asset_path(resource);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create asset directory {}", parent.display()))?;
    }

    let temp_path = temp_path_for(&path)?;
    let mut last_error = None;

    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        if let Err(error) = remove_file_if_exists(&temp_path) {
            diagnostics::record(
                Diagnostic::warning(
                    "resource.cleanup-failed",
                    "Stale temporary resource could not be removed",
                )
                .context("Path", temp_path.display())
                .reason(error)
                .suggestion("The download will continue and may replace the file later"),
            );
        }
        match download_resource_once(client, resource, &path, &temp_path) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < MAX_DOWNLOAD_ATTEMPTS => {
                let delay = backoff_delay(attempt);
                diagnostics::record(
                    Diagnostic::warning(
                        "resource.download-retry",
                        "Resource download attempt failed",
                    )
                    .context("Resource", resource.name)
                    .context("URL", resource.url)
                    .context("Destination", path.display())
                    .context("Attempt", format!("{attempt} of {MAX_DOWNLOAD_ATTEMPTS}"))
                    .context("Retry in", format!("{} seconds", delay.as_secs()))
                    .reason(format!("{error:#}"))
                    .suggestion("The download will be retried automatically"),
                );
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
) -> Result<()> {
    let mut response = send_with_retries(client, resource)?;
    let expected_length = response.content_length();
    let bytes = write_response(&mut response, temp_path)
        .with_context(|| format!("failed to write temporary asset {}", temp_path.display()))?;
    validate_download_size(resource, bytes, expected_length)?;
    // Both paths are in the asset directory; failed replacement leaves the cache intact.
    fs::rename(temp_path, path).with_context(|| {
        format!(
            "failed to move temporary asset {} into place at {}",
            temp_path.display(),
            path.display()
        )
    })?;

    Ok(())
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
                diagnostics::record(
                    Diagnostic::warning(
                        "resource.request-retry",
                        "Resource request will be retried",
                    )
                    .context("Resource", resource.name)
                    .context("URL", resource.url)
                    .context("Attempt", format!("{attempt} of {MAX_DOWNLOAD_ATTEMPTS}"))
                    .context("Retry in", format!("{} seconds", delay.as_secs()))
                    .reason(format!("HTTP {status}")),
                );
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
                diagnostics::record(
                    Diagnostic::warning(
                        "resource.request-retry",
                        "Resource request will be retried",
                    )
                    .context("Resource", resource.name)
                    .context("URL", resource.url)
                    .context("Attempt", format!("{attempt} of {MAX_DOWNLOAD_ATTEMPTS}"))
                    .context("Retry in", format!("{} seconds", delay.as_secs()))
                    .reason(format!("{:#}", anyhow::Error::new(error))),
                );
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

    if let Some(expected_length) = expected_length
        && bytes != expected_length
    {
        bail!(
            "downloaded {} bytes for {}, expected {}",
            bytes,
            resource.name,
            expected_length
        );
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

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ygo-resource-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn download_fixture(directory: &Path, body: &'static [u8], length: usize) -> Result<()> {
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}/cards.cdb", listener.local_addr()?);
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("missing resource request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = String::new();
            BufReader::new(&mut stream).read_line(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });
        let resource = Resource {
            name: "test cards",
            url: &url,
            path: &["cards.cdb"],
        };
        let path = directory.join("cards.cdb");
        let result = download_resource_once(&client, &resource, &path, &temp_path_for(&path)?);
        server.join().unwrap();
        result
    }

    #[test]
    fn refreshes_existing_cache_despite_an_obsolete_backup_path() {
        let directory = TestDirectory::new();
        let cached = directory.0.join("cards.cdb");
        let temporary = temp_path_for(&cached).unwrap();
        fs::write(&cached, b"old cache").unwrap();
        fs::create_dir(directory.0.join("cards.cdb.bak")).unwrap();

        download_fixture(&directory.0, b"new cache", 9).unwrap();

        assert_eq!(fs::read(&cached).unwrap(), b"new cache");
        assert!(!temporary.exists());
    }

    #[test]
    fn incomplete_download_preserves_existing_cache() {
        let directory = TestDirectory::new();
        let cached = directory.0.join("cards.cdb");
        fs::write(&cached, b"valid cache").unwrap();

        assert!(download_fixture(&directory.0, b"partial", 100).is_err());
        assert_eq!(fs::read(&cached).unwrap(), b"valid cache");
    }
}
