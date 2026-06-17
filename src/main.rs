use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use serde::Deserialize;
use ygo_cards::cards::{ImageFailure, ImageSummary, WriteReport};

const SUMMARY_REPORT: &str = "output/report.md";

fn main() -> Result<()> {
    let options = Options::parse()?;

    if options.refresh_resources {
        for resource in ygo_cards::resources::download_all()? {
            println!(
                "downloaded {:>8} bytes in {} attempt(s) -> {}",
                resource.bytes,
                resource.attempts,
                resource.path.display()
            );
        }
    } else {
        ygo_cards::resources::ensure_all()?;
    }

    let ot_report = ygo_cards::cards::ot::write_json(ygo_cards::cards::ot::BuildOptions {
        check_images: options.check_images,
    })?;
    print_write_report(&ot_report);

    let rd_report = ygo_cards::cards::rd::write_json(ygo_cards::cards::rd::BuildOptions {
        check_images: options.check_images,
    })?;
    print_write_report(&rd_report);

    let reports = [&ot_report, &rd_report];
    let latest_comparisons = compare_latest_release(&reports)?;
    let summary_path =
        write_summary_report(&reports, &latest_comparisons, Path::new(SUMMARY_REPORT))?;
    println!("summary report -> {}", summary_path.display());
    print_summary_report(&reports, &latest_comparisons);

    Ok(())
}

fn print_write_report(report: &WriteReport) {
    println!(
        "wrote {} cards (skipped {}) -> {}",
        report.cards_written,
        report.cards_skipped,
        report.path.display()
    );

    for summary in &report.lf_summaries {
        println!(
            "  {} lf: forbidden={} limit={} semi={} unlimited={}",
            summary.label,
            summary.counts[0],
            summary.counts[1],
            summary.counts[2],
            summary.counts[3]
        );
    }

    print_image_summary(report.image_summary);
    print_image_failures(&report.image_failures);
}

fn print_image_summary(summary: ImageSummary) {
    if !summary.enabled {
        return;
    }

    println!(
        "  images: success={} failed={} primary={} alias={} checked_cards={} unique_found={} unique_missing={} cache_hits={} network_errors={}",
        summary.successful_cards(),
        summary.missing,
        summary.primary_found,
        summary.alias_found,
        summary.cards_checked,
        summary.unique_urls_found,
        summary.unique_urls_missing,
        summary.cache_hits,
        summary.network_errors,
    );
}

fn print_image_failures(failures: &[ImageFailure]) {
    if failures.is_empty() {
        return;
    }

    println!("  image failed cards:");
    for failure in failures {
        println!(
            "    {} id={} alias={} name={}",
            failure.environment, failure.id, failure.alias, failure.name
        );
    }
}

fn write_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
    path: &Path,
) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }

    let text = build_summary_report(reports, latest_comparisons);
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn print_summary_report(reports: &[&WriteReport], latest_comparisons: &[LatestComparisonReport]) {
    let total_cards = reports
        .iter()
        .map(|report| report.cards_written)
        .sum::<usize>();
    let total_skipped = reports
        .iter()
        .map(|report| report.cards_skipped)
        .sum::<usize>();
    let image_summary = total_image_summary(reports);
    let image_failures = reports
        .iter()
        .map(|report| report.image_failures.len())
        .sum::<usize>();

    println!("summary:");
    println!("  cards: written={} skipped={}", total_cards, total_skipped);
    if image_summary.enabled {
        println!(
            "  images: success={} failed={} network_errors={}",
            image_summary.successful_cards(),
            image_failures,
            image_summary.network_errors
        );
    }

    for comparison in latest_comparisons {
        match &comparison.status {
            LatestComparisonStatus::Compared { previous_cards } => {
                println!(
                    "  {} new cards since latest: {} (previous={} current={})",
                    comparison.label,
                    comparison.added_cards.len(),
                    previous_cards,
                    comparison.current_cards
                );
            }
            LatestComparisonStatus::NotFound => {
                println!(
                    "  {} new cards since latest: skipped (latest asset not found)",
                    comparison.label
                );
            }
            LatestComparisonStatus::Unavailable(reason) => {
                println!(
                    "  {} new cards since latest: skipped ({})",
                    comparison.label, reason
                );
            }
        }
    }
}

fn build_summary_report(
    reports: &[&WriteReport],
    latest_comparisons: &[LatestComparisonReport],
) -> String {
    let mut report = String::new();
    let total_cards = reports
        .iter()
        .map(|report| report.cards_written)
        .sum::<usize>();
    let total_skipped = reports
        .iter()
        .map(|report| report.cards_skipped)
        .sum::<usize>();
    let image_summary = total_image_summary(reports);

    report.push_str("# ygo-cards Build Report\n\n");
    report.push_str("## Overview\n\n");
    report.push_str("| Metric | Value |\n");
    report.push_str("| --- | ---: |\n");
    report.push_str(&format!("| Cards written | {} |\n", total_cards));
    report.push_str(&format!("| Cards skipped | {} |\n", total_skipped));
    report.push_str(&format!(
        "| Image check | {} |\n\n",
        if image_summary.enabled {
            "enabled"
        } else {
            "disabled"
        }
    ));

    for environment_report in reports {
        report.push_str(&format!("## {}\n\n", environment_report.label));
        report.push_str("| Metric | Value |\n");
        report.push_str("| --- | ---: |\n");
        report.push_str(&format!(
            "| Output | `{}` |\n| Cards written | {} |\n| Cards skipped | {} |\n\n",
            environment_report.path.display(),
            environment_report.cards_written,
            environment_report.cards_skipped
        ));

        report.push_str("### Forbidden Lists\n\n");
        report.push_str("| List | Forbidden | Limit | Semi | Unlimited |\n");
        report.push_str("| --- | ---: | ---: | ---: | ---: |\n");
        for summary in &environment_report.lf_summaries {
            report.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                summary.label,
                summary.counts[0],
                summary.counts[1],
                summary.counts[2],
                summary.counts[3]
            ));
        }
        report.push('\n');

        if environment_report.image_summary.enabled {
            report.push_str("### Images\n\n");
            report.push_str("| Metric | Value |\n");
            report.push_str("| --- | ---: |\n");
            report.push_str(&format!(
                "| Successful cards | {} |\n| Failed cards | {} |\n| Primary hits | {} |\n| Alias hits | {} |\n| Checked cards | {} |\n| Unique URLs found | {} |\n| Unique URLs missing | {} |\n| Cache hits | {} |\n| Network errors | {} |\n\n",
                environment_report.image_summary.successful_cards(),
                environment_report.image_failures.len(),
                environment_report.image_summary.primary_found,
                environment_report.image_summary.alias_found,
                environment_report.image_summary.cards_checked,
                environment_report.image_summary.unique_urls_found,
                environment_report.image_summary.unique_urls_missing,
                environment_report.image_summary.cache_hits,
                environment_report.image_summary.network_errors,
            ));
            append_image_failures(&mut report, &environment_report.image_failures);
        }
    }

    if image_summary.enabled {
        report.push_str("## Image Totals\n\n");
        report.push_str("| Metric | Value |\n");
        report.push_str("| --- | ---: |\n");
        report.push_str(&format!(
            "| Successful cards | {} |\n| Failed cards | {} |\n| Primary hits | {} |\n| Alias hits | {} |\n| Checked cards | {} |\n| Unique URLs found | {} |\n| Unique URLs missing | {} |\n| Cache hits | {} |\n| Network errors | {} |\n",
            image_summary.successful_cards(),
            reports
                .iter()
                .map(|report| report.image_failures.len())
                .sum::<usize>(),
            image_summary.primary_found,
            image_summary.alias_found,
            image_summary.cards_checked,
            image_summary.unique_urls_found,
            image_summary.unique_urls_missing,
            image_summary.cache_hits,
            image_summary.network_errors
        ));
    }

    append_latest_comparison_report(&mut report, latest_comparisons);

    report
}

fn append_image_failures(report: &mut String, failures: &[ImageFailure]) {
    if failures.is_empty() {
        return;
    }

    report.push_str("#### Failed Image Cards\n\n");
    report.push_str("| Environment | ID | Alias | Name |\n");
    report.push_str("| --- | ---: | ---: | --- |\n");
    for failure in failures {
        report.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            failure.environment,
            failure.id,
            failure.alias,
            escape_markdown_cell(&failure.name)
        ));
    }
    report.push('\n');
}

fn escape_markdown_cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "<br>")
}

fn compare_latest_release(reports: &[&WriteReport]) -> Result<Vec<LatestComparisonReport>> {
    let url_config = ygo_cards::urls::urls()?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build latest release HTTP client")?;

    reports
        .iter()
        .map(|report| compare_latest_release_file(&client, url_config, report))
        .collect()
}

fn compare_latest_release_file(
    client: &reqwest::blocking::Client,
    url_config: &ygo_cards::urls::UrlConfig,
    report: &WriteReport,
) -> Result<LatestComparisonReport> {
    let latest_url = url_config.latest_json_url(report.label)?.to_string();
    let current_cards = read_card_summaries(&report.path)
        .with_context(|| format!("failed to read current {} cards", report.label))?;

    let (status, added_cards) = match fetch_latest_card_summaries(client, &latest_url) {
        LatestCardsFetch::Cards(previous_cards) => {
            let added_cards = find_added_cards(&current_cards, &previous_cards);
            (
                LatestComparisonStatus::Compared {
                    previous_cards: previous_cards.len(),
                },
                added_cards,
            )
        }
        LatestCardsFetch::NotFound => (LatestComparisonStatus::NotFound, Vec::new()),
        LatestCardsFetch::Unavailable(reason) => {
            (LatestComparisonStatus::Unavailable(reason), Vec::new())
        }
    };

    Ok(LatestComparisonReport {
        label: report.label,
        latest_url,
        current_cards: current_cards.len(),
        status,
        added_cards,
    })
}

fn fetch_latest_card_summaries(client: &reqwest::blocking::Client, url: &str) -> LatestCardsFetch {
    let response = match client.get(url).send() {
        Ok(response) => response,
        Err(error) => return LatestCardsFetch::Unavailable(format!("download failed: {error}")),
    };

    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return LatestCardsFetch::NotFound,
        status => return LatestCardsFetch::Unavailable(format!("HTTP {status}")),
    }

    let text = match response.text() {
        Ok(text) => text,
        Err(error) => return LatestCardsFetch::Unavailable(format!("read failed: {error}")),
    };

    match parse_card_summaries(&text) {
        Ok(cards) => LatestCardsFetch::Cards(cards),
        Err(error) => LatestCardsFetch::Unavailable(format!("invalid JSON: {error}")),
    }
}

fn read_card_summaries(path: &Path) -> Result<Vec<CardSummary>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read cards JSON {}", path.display()))?;
    parse_card_summaries(&text)
}

fn parse_card_summaries(text: &str) -> Result<Vec<CardSummary>> {
    serde_json::from_str(text).context("failed to parse card summaries")
}

fn find_added_cards(
    current_cards: &[CardSummary],
    previous_cards: &[CardSummary],
) -> Vec<CardSummary> {
    let previous_ids = previous_cards
        .iter()
        .map(|card| card.id)
        .collect::<BTreeSet<_>>();

    current_cards
        .iter()
        .filter(|card| !previous_ids.contains(&card.id))
        .cloned()
        .collect()
}

fn append_latest_comparison_report(
    report: &mut String,
    latest_comparisons: &[LatestComparisonReport],
) {
    report.push_str("## New Cards Since Latest Release\n\n");

    for comparison in latest_comparisons {
        report.push_str(&format!("### {}\n\n", comparison.label));
        report.push_str(&format!(
            "Compared against [{}]({}).\n\n",
            comparison.latest_url, comparison.latest_url
        ));

        match &comparison.status {
            LatestComparisonStatus::Compared { previous_cards } => {
                report.push_str("| Metric | Value |\n");
                report.push_str("| --- | ---: |\n");
                report.push_str(&format!("| Previous cards | {} |\n", previous_cards));
                report.push_str(&format!(
                    "| Current cards | {} |\n",
                    comparison.current_cards
                ));
                report.push_str(&format!(
                    "| New cards | {} |\n\n",
                    comparison.added_cards.len()
                ));

                if comparison.added_cards.is_empty() {
                    report.push_str("No new cards.\n\n");
                } else {
                    report.push_str("| ID | Name | Type |\n");
                    report.push_str("| ---: | --- | --- |\n");
                    for card in &comparison.added_cards {
                        report.push_str(&format!(
                            "| {} | {} | {} |\n",
                            card.id,
                            escape_markdown_cell(&card.name),
                            escape_markdown_cell(&card_type_display(&card.card_type))
                        ));
                    }
                    report.push('\n');
                }
            }
            LatestComparisonStatus::NotFound => {
                report.push_str("Previous latest file was not found; comparison skipped.\n\n");
            }
            LatestComparisonStatus::Unavailable(reason) => {
                report.push_str(&format!(
                    "Previous latest file could not be compared: {}.\n\n",
                    escape_markdown_cell(reason)
                ));
            }
        }
    }
}

fn card_type_display(card_type: &[String]) -> String {
    if card_type.is_empty() {
        String::from("-")
    } else {
        card_type.join("/")
    }
}

fn total_image_summary(reports: &[&WriteReport]) -> ImageSummary {
    reports
        .iter()
        .fold(ImageSummary::default(), |mut total, report| {
            let summary = report.image_summary;
            total.enabled |= summary.enabled;
            total.cards_checked += summary.cards_checked;
            total.primary_found += summary.primary_found;
            total.alias_found += summary.alias_found;
            total.missing += summary.missing;
            total.unique_urls_found += summary.unique_urls_found;
            total.unique_urls_missing += summary.unique_urls_missing;
            total.network_errors += summary.network_errors;
            total.cache_hits += summary.cache_hits;
            total
        })
}

#[derive(Debug, Default)]
struct Options {
    refresh_resources: bool,
    check_images: bool,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self::default();

        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--refresh-resources" => options.refresh_resources = true,
                "--check-images" => options.check_images = true,
                _ => bail!("unknown option: {arg}"),
            }
        }

        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: i64, name: &str, card_type: &[&str]) -> CardSummary {
        CardSummary {
            id,
            name: name.to_string(),
            card_type: card_type.iter().map(|value| value.to_string()).collect(),
        }
    }

    #[test]
    fn parses_card_summaries_from_generated_json_shape() {
        let cards = parse_card_summaries(
            r#"[
                {"id":89631139,"name":"Blue-Eyes White Dragon","type":["怪兽","龙族","通常"],"atk":3000},
                {"id":5318639,"name":"Mystical Space Typhoon","type":["魔法","速攻"]}
            ]"#,
        )
        .unwrap();

        assert_eq!(
            cards,
            vec![
                card(
                    89631139,
                    "Blue-Eyes White Dragon",
                    &["怪兽", "龙族", "通常"]
                ),
                card(5318639, "Mystical Space Typhoon", &["魔法", "速攻"]),
            ]
        );
    }

    #[test]
    fn finds_added_cards_by_id() {
        let current_cards = vec![
            card(1, "Existing", &["魔法"]),
            card(2, "New", &["怪兽", "龙族", "通常"]),
        ];
        let previous_cards = vec![card(1, "Old Name", &["陷阱"])];

        assert_eq!(
            find_added_cards(&current_cards, &previous_cards),
            vec![card(2, "New", &["怪兽", "龙族", "通常"])]
        );
    }

    #[test]
    fn formats_card_types_with_slashes() {
        assert_eq!(
            card_type_display(&["怪兽".to_string(), "龙族".to_string(), "通常".to_string()]),
            "怪兽/龙族/通常"
        );
        assert_eq!(card_type_display(&[]), "-");
    }

    #[test]
    fn appends_added_cards_to_latest_comparison_report() {
        let mut report = String::new();
        append_latest_comparison_report(
            &mut report,
            &[LatestComparisonReport {
                label: "OT",
                latest_url: ygo_cards::urls::urls()
                    .unwrap()
                    .latest_json_url("OT")
                    .unwrap()
                    .to_string(),
                current_cards: 2,
                status: LatestComparisonStatus::Compared { previous_cards: 1 },
                added_cards: vec![card(2, "New|Card", &["怪兽", "龙族", "通常"])],
            }],
        );

        assert!(report.contains("## New Cards Since Latest Release"));
        assert!(report.contains("| New cards | 1 |"));
        assert!(report.contains("| 2 | New\\|Card | 怪兽/龙族/通常 |"));
    }
}

#[derive(Debug)]
struct LatestComparisonReport {
    label: &'static str,
    latest_url: String,
    current_cards: usize,
    status: LatestComparisonStatus,
    added_cards: Vec<CardSummary>,
}

#[derive(Debug)]
enum LatestComparisonStatus {
    Compared { previous_cards: usize },
    NotFound,
    Unavailable(String),
}

enum LatestCardsFetch {
    Cards(Vec<CardSummary>),
    NotFound,
    Unavailable(String),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct CardSummary {
    id: i64,
    name: String,
    #[serde(default, rename = "type")]
    card_type: Vec<String>,
}
