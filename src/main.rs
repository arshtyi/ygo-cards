use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
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
    let summary_path = write_summary_report(&reports, Path::new(SUMMARY_REPORT))?;
    println!("summary report -> {}", summary_path.display());
    print_summary_report(&reports);

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

fn write_summary_report(reports: &[&WriteReport], path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create report directory {}", parent.display()))?;
    }

    let text = build_summary_report(reports);
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn print_summary_report(reports: &[&WriteReport]) {
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
}

fn build_summary_report(reports: &[&WriteReport]) -> String {
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
