use anyhow::{Result, bail};
use ygo_cards::cards::{ImageSummary, WriteReport};

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

    let report = ygo_cards::cards::ot::write_json(ygo_cards::cards::ot::BuildOptions {
        check_images: options.check_images,
    })?;
    print_write_report(&report);

    let report = ygo_cards::cards::rd::write_json(ygo_cards::cards::rd::BuildOptions {
        check_images: options.check_images,
    })?;
    print_write_report(&report);

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
