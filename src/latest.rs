use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use anyhow::{Context, Result, ensure};
use reqwest::StatusCode;
use serde_json::Value;
use similar::TextDiff;

use crate::{cards::DatasetReport, cli::Options, endpoints, environment::Environment};

pub(crate) fn compare_latest_release(
    reports: &[&DatasetReport],
    options: &Options,
) -> Result<LatestReleaseComparison> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build latest release HTTP client")?;
    let endpoints = endpoints::endpoints()?;
    // Resolve latest only once, before fetching either dataset.
    let release = if options.no_previous_release {
        None
    } else if options.previous_release_dir.is_some() {
        options
            .previous_release_url
            .as_deref()
            .map(release_reference)
            .transpose()?
    } else {
        resolve_latest_release(&client, endpoints.latest_release_url())?
    };

    let datasets = reports
        .iter()
        .map(|report| {
            let current = read_cards(&report.path)?;
            let previous = if let Some(directory) = &options.previous_release_dir {
                Some(read_cards(
                    &directory.join(dataset_filename(report.environment)),
                )?)
            } else if let Some(release) = &release {
                let url = pinned_dataset_url(
                    endpoints.published_dataset_url(report.environment),
                    release,
                )?;
                let text = client
                    .get(&url)
                    .send()
                    .with_context(|| format!("failed to download previous dataset {url}"))?
                    .error_for_status()
                    .with_context(|| format!("previous release dataset is unavailable: {url}"))?
                    .text()?;
                Some(
                    parse_cards(&text)
                        .with_context(|| format!("invalid previous dataset {url}"))?,
                )
            } else {
                None
            };
            compare_cards(report.environment, &current, previous.as_ref())
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(LatestReleaseComparison { release, datasets })
}

fn resolve_latest_release(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<Option<ReleaseReference>> {
    let response = client
        .head(url)
        .send()
        .context("failed to resolve latest release")?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response
        .error_for_status()
        .context("latest release is unavailable")?;
    release_reference(response.url().as_str()).map(Some)
}

fn release_reference(url: &str) -> Result<ReleaseReference> {
    let mut url = reqwest::Url::parse(url).context("invalid previous release URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "release URL must use HTTP or HTTPS"
    );
    let tag = release_tag_from_path(url.path())
        .context("expected a fixed /releases/tag/ URL")?
        .to_string();
    url.set_query(None);
    url.set_fragment(None);
    Ok(ReleaseReference {
        tag,
        url: url.to_string(),
    })
}

fn release_tag_from_path(path: &str) -> Option<&str> {
    let (_, tag) = path.split_once("/releases/tag/")?;
    (!tag.is_empty()).then_some(tag)
}

fn pinned_dataset_url(url: &str, release: &ReleaseReference) -> Result<String> {
    ensure!(
        url.contains("/releases/latest/download/"),
        "dataset URL cannot be pinned to a release: {url}"
    );
    Ok(url.replacen(
        "/releases/latest/download/",
        &format!("/releases/download/{}/", release.tag),
        1,
    ))
}

fn dataset_filename(environment: Environment) -> &'static str {
    match environment {
        Environment::Ot => "ot.json",
        Environment::Rd => "rd.json",
    }
}

type Cards = BTreeMap<i64, Value>;

fn read_cards(path: &Path) -> Result<Cards> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read cards JSON {}", path.display()))?;
    parse_cards(&text).with_context(|| format!("invalid cards JSON {}", path.display()))
}

fn parse_cards(text: &str) -> Result<Cards> {
    let values: Vec<Value> =
        serde_json::from_str(text).context("expected a JSON array of cards")?;
    let mut cards = Cards::new();
    for value in values {
        let id = value
            .get("id")
            .and_then(Value::as_i64)
            .context("card must have an integer ID")?;
        ensure!(
            value.get("name").and_then(Value::as_str).is_some(),
            "card {id} must have a string name"
        );
        ensure!(cards.insert(id, value).is_none(), "duplicate card ID {id}");
    }
    Ok(cards)
}

// Matches the dataset writer: sorted IDs and object keys, two-space indentation,
// original array order, and one final newline. Formatting changes produce no diff.
fn canonical_json(cards: &Cards) -> Result<String> {
    Ok(serde_json::to_string_pretty(&cards.values().collect::<Vec<_>>())? + "\n")
}

fn compare_cards(
    environment: Environment,
    current: &Cards,
    previous: Option<&Cards>,
) -> Result<LatestComparisonReport> {
    let mut report = LatestComparisonReport {
        environment,
        current_cards: current.len(),
        status: LatestComparisonStatus::NotFound,
        added_cards: 0,
        modified_cards: 0,
        removed_cards: 0,
        diff: String::new(),
    };
    let Some(previous) = previous else {
        return Ok(report);
    };
    report.status = LatestComparisonStatus::Compared {
        previous_cards: previous.len(),
    };
    for (id, card) in current {
        match previous.get(id) {
            None => report.added_cards += 1,
            Some(old) if old != card => report.modified_cards += 1,
            _ => {}
        }
    }
    report.removed_cards = previous
        .keys()
        .filter(|id| !current.contains_key(id))
        .count();
    if report.has_changes() {
        let before = canonical_json(previous)?;
        let after = canonical_json(current)?;
        // A bounded computation still yields an exact patch on very large rewrites.
        let diff = TextDiff::configure()
            .timeout(Duration::from_secs(5))
            .diff_lines(&before, &after);
        let filename = dataset_filename(environment);
        report.diff = diff
            .unified_diff()
            .context_radius(3)
            .header(&format!("a/{filename}"), &format!("b/{filename}"))
            .to_string();
    }
    Ok(report)
}

#[derive(Debug)]
pub(crate) struct LatestReleaseComparison {
    pub(crate) release: Option<ReleaseReference>,
    pub(crate) datasets: Vec<LatestComparisonReport>,
}

impl LatestReleaseComparison {
    pub(crate) fn should_publish(&self) -> bool {
        self.datasets.iter().any(|dataset| {
            matches!(dataset.status, LatestComparisonStatus::NotFound) || dataset.has_changes()
        })
    }

    pub(crate) fn write_artifacts(&self, output_dir: &Path) -> Result<()> {
        remove_stale_diff(&output_dir.join("data.diff"))?;
        for dataset in &self.datasets {
            let diff_path = output_dir.join(dataset.diff_filename());
            if dataset.diff.is_empty() {
                remove_stale_diff(&diff_path)?;
            } else {
                fs::write(&diff_path, &dataset.diff)
                    .with_context(|| format!("failed to write {}", diff_path.display()))?;
            }
        }
        fs::write(
            output_dir.join("release-changed"),
            if self.should_publish() {
                "true\n"
            } else {
                "false\n"
            },
        )
        .context("failed to write release publication decision")
    }
}

fn remove_stale_diff(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to remove stale diff {}", path.display()))
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReleaseReference {
    pub(crate) tag: String,
    pub(crate) url: String,
}

#[derive(Debug)]
pub(crate) struct LatestComparisonReport {
    pub(crate) environment: Environment,
    pub(crate) current_cards: usize,
    pub(crate) status: LatestComparisonStatus,
    pub(crate) added_cards: usize,
    pub(crate) modified_cards: usize,
    pub(crate) removed_cards: usize,
    pub(crate) diff: String,
}

impl LatestComparisonReport {
    pub(crate) fn diff_filename(&self) -> &'static str {
        match self.environment {
            Environment::Ot => "ot.diff",
            Environment::Rd => "rd.diff",
        }
    }

    fn has_changes(&self) -> bool {
        self.added_cards + self.modified_cards + self.removed_cards > 0
    }
}

#[derive(Debug)]
pub(crate) enum LatestComparisonStatus {
    Compared { previous_cards: usize },
    NotFound,
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        process::Command,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::cards::ImageCheckSummary;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "ygo-release-test-{}-{}",
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

    fn comparison(previous: &str, current: &str) -> LatestComparisonReport {
        compare_cards(
            Environment::Ot,
            &parse_cards(current).unwrap(),
            Some(&parse_cards(previous).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn ignores_formatting_key_order_and_card_order() {
        let report = comparison(
            r#"[{"id":2,"name":"二","nested":{"b":2,"a":1}},{"id":1,"name":"一"}]"#,
            "[\n  {\"name\":\"一\",\"id\":1}, {\"nested\":{\"a\":1,\"b\":2},\"name\":\"二\",\"id\":2}\n]",
        );
        assert!(!report.has_changes());
        assert!(report.diff.is_empty());
        assert!(
            !LatestReleaseComparison {
                release: None,
                datasets: vec![report]
            }
            .should_publish()
        );
    }

    #[test]
    fn detects_nested_values_array_order_and_absent_versus_null() {
        for (before, after) in [
            (r#""extra":{"a":null}"#, r#""extra":{}"#),
            (r#""extra":null"#, r#""extra":false"#),
            (r#""extra":[1,2]"#, r#""extra":[2,1]"#),
            (r#""extra":{"a":1}"#, r#""extra":{"a":2}"#),
        ] {
            let report = comparison(
                &format!(r#"[{{"id":1,"name":"卡",{before}}}]"#),
                &format!(r#"[{{"id":1,"name":"卡",{after}}}]"#),
            );
            assert_eq!(report.modified_cards, 1);
            assert_eq!(report.added_cards + report.removed_cards, 0);
            assert!(!report.diff.is_empty());
        }
        let report = comparison(
            r#"[{"id":1,"name":"卡"}]"#,
            r#"[{"id":1,"name":"卡","extra":null}]"#,
        );
        assert_eq!(report.modified_cards, 1);
        assert!(report.diff.contains("+    \"extra\": null,"));
    }

    #[test]
    fn separate_patches_reconstruct_only_their_own_dataset() {
        let previous = parse_cards(
            r#"[
            {"id":1,"name":"改名","atk":100,"text":"旧\n描述"},
            {"id":2,"name":"删除"},
            {"id":4,"name":"不变","type":["怪兽","通常"]}
        ]"#,
        )
        .unwrap();
        let current = parse_cards(
            r#"[
            {"id":1,"name":"新名","atk":200,"text":"新\n描述"},
            {"id":3,"name":"新增","extra":null},
            {"id":4,"name":"不变","type":["怪兽","通常"]}
        ]"#,
        )
        .unwrap();
        let directory = TestDirectory::new();
        let mut datasets = Vec::new();
        for environment in [Environment::Ot, Environment::Rd] {
            let report = compare_cards(environment, &current, Some(&previous)).unwrap();
            assert_eq!(
                (
                    report.added_cards,
                    report.modified_cards,
                    report.removed_cards
                ),
                (1, 1, 1)
            );
            fs::write(
                directory.0.join(dataset_filename(environment)),
                canonical_json(&previous).unwrap(),
            )
            .unwrap();
            datasets.push(report);
        }
        let release = LatestReleaseComparison {
            release: None,
            datasets,
        };
        release.write_artifacts(&directory.0).unwrap();
        assert!(!directory.0.join("data.diff").exists());
        for (index, report) in release.datasets.iter().enumerate() {
            let diff = fs::read_to_string(directory.0.join(report.diff_filename())).unwrap();
            assert_eq!(diff.matches("--- a/").count(), 1);
            let result = Command::new("git")
                .args(["apply", report.diff_filename()])
                .current_dir(&directory.0)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                fs::read_to_string(directory.0.join("ot.json")).unwrap(),
                canonical_json(&current).unwrap()
            );
            assert_eq!(
                fs::read_to_string(directory.0.join("rd.json")).unwrap(),
                canonical_json(if index == 0 { &previous } else { &current }).unwrap()
            );
        }
    }

    #[test]
    fn empty_existing_dataset_is_a_baseline_but_first_release_is_not() {
        let cards = parse_cards(r#"[{"id":1,"name":"卡"}]"#).unwrap();
        let first = compare_cards(Environment::Ot, &cards, None).unwrap();
        assert_eq!(first.added_cards, 0);
        assert!(first.diff.is_empty());
        assert!(matches!(first.status, LatestComparisonStatus::NotFound));
        assert!(
            LatestReleaseComparison {
                release: None,
                datasets: vec![first]
            }
            .should_publish()
        );
        let added = compare_cards(Environment::Ot, &cards, Some(&Cards::new())).unwrap();
        assert_eq!(added.added_cards, 1);
        assert!(!added.diff.is_empty());
        let removed = compare_cards(Environment::Ot, &Cards::new(), Some(&cards)).unwrap();
        assert_eq!(removed.removed_cards, 1);
        assert!(!removed.diff.is_empty());
    }

    #[test]
    fn writes_complete_artifact_and_removes_stale_diff() {
        let directory = TestDirectory::new();
        let diff_path = directory.0.join("ot.diff");
        let changed_path = directory.0.join("release-changed");
        let previous = parse_cards(r#"[{"id":1,"name":"旧"}]"#).unwrap();
        let current = parse_cards(&format!(
            r#"[{{"id":1,"name":"新","text":"{}"}}]"#,
            "很长的描述".repeat(10_000)
        ))
        .unwrap();
        let changed = compare_cards(Environment::Ot, &current, Some(&previous)).unwrap();
        let expected = changed.diff.clone();
        let release = LatestReleaseComparison {
            release: None,
            datasets: vec![changed],
        };
        release.write_artifacts(&directory.0).unwrap();
        assert_eq!(fs::read_to_string(&diff_path).unwrap(), expected);
        assert_eq!(fs::read_to_string(&changed_path).unwrap(), "true\n");
        for baseline in [None, Some(&current)] {
            fs::write(&diff_path, "stale").unwrap();
            let release = LatestReleaseComparison {
                release: None,
                datasets: vec![compare_cards(Environment::Ot, &current, baseline).unwrap()],
            };
            release.write_artifacts(&directory.0).unwrap();
            assert!(!diff_path.exists());
            assert_eq!(
                fs::read_to_string(&changed_path).unwrap().trim(),
                if baseline.is_some() { "false" } else { "true" }
            );
        }
    }

    #[test]
    fn writes_only_changed_dataset_diffs_and_clears_other_stale_files() {
        let directory = TestDirectory::new();
        let before = parse_cards(r#"[{"id":1,"name":"旧"}]"#).unwrap();
        let after = parse_cards(r#"[{"id":1,"name":"新"}]"#).unwrap();
        for changed_environment in [Environment::Ot, Environment::Rd] {
            for filename in ["ot.diff", "rd.diff", "data.diff"] {
                fs::write(directory.0.join(filename), "stale").unwrap();
            }
            let datasets = [Environment::Ot, Environment::Rd]
                .into_iter()
                .map(|environment| {
                    compare_cards(
                        environment,
                        if environment == changed_environment {
                            &after
                        } else {
                            &before
                        },
                        Some(&before),
                    )
                    .unwrap()
                })
                .collect();
            let release = LatestReleaseComparison {
                release: None,
                datasets,
            };
            release.write_artifacts(&directory.0).unwrap();
            for report in &release.datasets {
                assert_eq!(
                    directory.0.join(report.diff_filename()).exists(),
                    report.environment == changed_environment
                );
            }
            assert!(!directory.0.join("data.diff").exists());
            assert_eq!(
                fs::read_to_string(directory.0.join("release-changed")).unwrap(),
                "true\n"
            );
        }
    }

    #[test]
    fn rejects_invalid_baselines_instead_of_treating_them_as_first_release() {
        for text in [
            "{}",
            "null",
            "[{}]",
            r#"[{"id":"1","name":"卡"}]"#,
            r#"[{"id":1,"name":"甲"},{"id":1,"name":"乙"}]"#,
        ] {
            assert!(parse_cards(text).is_err(), "accepted {text}");
        }
        let directory = TestDirectory::new();
        let path = directory.0.join("current.json");
        fs::write(&path, "[]").unwrap();
        let report = DatasetReport {
            environment: Environment::Ot,
            path,
            cards_written: 0,
            cards_skipped: 0,
            image_summary: ImageCheckSummary::default(),
            image_failures: vec![],
        };
        let options = Options {
            previous_release_dir: Some(directory.0.clone()),
            ..Options::default()
        };
        assert!(compare_latest_release(&[&report], &options).is_err());
        fs::write(directory.0.join("ot.json"), "broken JSON").unwrap();
        assert!(compare_latest_release(&[&report], &options).is_err());
        fs::write(directory.0.join("ot.json"), "[]").unwrap();
        assert!(
            !compare_latest_release(&[&report], &options)
                .unwrap()
                .should_publish()
        );
    }

    #[test]
    fn pins_downloads_to_the_resolved_tag() {
        let release =
            release_reference("https://github.com/arshtyi/ygo-cards/releases/tag/0.0.2?x=1#test")
                .unwrap();
        assert_eq!(release.tag, "0.0.2");
        assert_eq!(
            release.url,
            "https://github.com/arshtyi/ygo-cards/releases/tag/0.0.2"
        );
        assert_eq!(
            pinned_dataset_url(
                "https://github.com/arshtyi/ygo-cards/releases/latest/download/ot.json",
                &release
            )
            .unwrap(),
            "https://github.com/arshtyi/ygo-cards/releases/download/0.0.2/ot.json"
        );
        assert!(release_reference("https://github.com/arshtyi/ygo-cards/releases/latest").is_err());
        assert!(pinned_dataset_url("https://example.test/ot.json", &release).is_err());
    }
}
