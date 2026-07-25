mod cli;
mod latest;
mod report;

use std::{path::Path, process::ExitCode};

use anyhow::Result;
use clap::error::ErrorKind;

use cli::Options;
use latest::compare_latest_release;
use report::{
    SUMMARY_REPORT, print_summary_report, print_write_report, write_summary_report,
};
use ygo_cards::diagnostics::{self, BUILD_LOG_PATH};

fn main() -> ExitCode {
    let options = match Options::try_parse() {
        Ok(options) => options,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            if diagnostics::init().is_ok() {
                diagnostics::error(format_args!(
                    "invalid command-line arguments:\n{}",
                    error.to_string().trim()
                ));
                let _ = diagnostics::finish();
                println!("build failed; log -> {BUILD_LOG_PATH}");
            }
            return ExitCode::from(2);
        }
    };

    let log_path = match diagnostics::init() {
        Ok(path) => path,
        Err(_) => return ExitCode::FAILURE,
    };
    diagnostics::install_panic_hook();

    match run(options) {
        Ok(()) => match diagnostics::finish() {
            Ok(()) => {
                println!("build log -> {}", log_path.display());
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::FAILURE,
        },
        Err(error) => {
            diagnostics::error(format_args!("build failed: {error:#}"));
            let _ = diagnostics::finish();
            println!("build failed; log -> {}", log_path.display());
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<()> {
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
