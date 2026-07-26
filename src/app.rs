use std::{path::Path, process::ExitCode};

use anyhow::Result;
use clap::error::ErrorKind;
use ygo_cards::{
    cards::{BuildOptions, LfStatisticsOptions},
    diagnostics::{self, BUILD_LOG_PATH},
};

use crate::{
    cli::Options,
    latest::compare_latest_release,
    report::{
        SUMMARY_REPORT, print_summary_report, print_write_report, write_summary_report,
    },
};

pub(crate) fn execute() -> ExitCode {
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
        Err(error) => return command_line_failure(error),
    };

    let log_path = match diagnostics::init() {
        Ok(path) => path,
        Err(_) => return ExitCode::FAILURE,
    };
    diagnostics::install_panic_hook();

    match generate(options) {
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

fn command_line_failure(error: clap::Error) -> ExitCode {
    if diagnostics::init().is_ok() {
        diagnostics::error(format_args!(
            "invalid command-line arguments:\n{}",
            error.to_string().trim()
        ));
        let _ = diagnostics::finish();
        println!("build failed; log -> {BUILD_LOG_PATH}");
    }
    ExitCode::from(2)
}

fn generate(options: Options) -> Result<()> {
    prepare_resources(options.refresh_resources)?;

    let build_options = BuildOptions {
        check_images: options.check_images,
        skip_image_failures: options.skip_image_failures,
    };
    let lf_statistics_options = LfStatisticsOptions {
        ignore_aliases: !options.include_aliases_in_lf_statistics,
    };
    let ot_report =
        ygo_cards::cards::ot::write_json_with_lf_statistics(build_options, lf_statistics_options)?;
    print_write_report(&ot_report);

    let rd_report =
        ygo_cards::cards::rd::write_json_with_lf_statistics(build_options, lf_statistics_options)?;
    print_write_report(&rd_report);

    let reports = [&ot_report, &rd_report];
    let latest_comparisons = compare_latest_release(&reports)?;
    let summary_path =
        write_summary_report(&reports, &latest_comparisons, Path::new(SUMMARY_REPORT))?;
    println!("summary report -> {}", summary_path.display());
    print_summary_report(&reports, &latest_comparisons);

    Ok(())
}

fn prepare_resources(refresh: bool) -> Result<()> {
    if !refresh {
        return ygo_cards::resources::ensure_all();
    }

    for resource in ygo_cards::resources::download_all()? {
        println!(
            "downloaded {:>8} bytes in {} attempt(s) -> {}",
            resource.bytes,
            resource.attempts,
            resource.path.display()
        );
    }
    Ok(())
}
