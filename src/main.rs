mod cli;
mod latest;
mod report;

use std::path::Path;

use anyhow::Result;

use cli::Options;
use latest::compare_latest_release;
use report::{
    SUMMARY_REPORT, print_summary_report, print_write_report, write_summary_report,
};

fn main() -> Result<()> {
    let options = Options::parse();

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
        skip_image_failures: options.skip_image_failures,
    })?;
    print_write_report(&ot_report);

    let rd_report = ygo_cards::cards::rd::write_json(ygo_cards::cards::rd::BuildOptions {
        check_images: options.check_images,
        skip_image_failures: options.skip_image_failures,
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
