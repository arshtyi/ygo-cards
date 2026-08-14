use std::{path::Path, process::ExitCode};

use anyhow::Result;
use clap::error::ErrorKind;

use crate::{
    cards::{self, GenerationOptions},
    cli::Options,
    diagnostics::{self, BUILD_LOG_PATH, Diagnostic},
    formatting::{format_count, plural},
    latest::compare_latest_release,
    report::{SUMMARY_REPORT, print_dataset_report, print_summary_report, write_summary_report},
    resources,
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
        Err(error) => {
            print_error("Could not initialize build diagnostics", &error, None);
            return ExitCode::FAILURE;
        }
    };
    diagnostics::install_panic_hook();

    match generate(options) {
        Ok(()) => match diagnostics::finish() {
            Ok(()) => {
                let diagnostic_summary = diagnostics::snapshot()
                    .map(|snapshot| {
                        if snapshot.is_clean() {
                            String::from("clean")
                        } else {
                            format!(
                                "{} {}, {} {}",
                                snapshot.warnings(),
                                plural(snapshot.warnings(), "warning", "warnings"),
                                snapshot.errors(),
                                plural(snapshot.errors(), "error", "errors")
                            )
                        }
                    })
                    .unwrap_or_else(|_| String::from("summary unavailable"));
                println!(
                    "\nBuild complete\n  {:<12} {}\n  {:<12} {} ({})",
                    "Report",
                    SUMMARY_REPORT,
                    "Diagnostics",
                    log_path.display(),
                    diagnostic_summary
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                print_error(
                    "Build completed, but diagnostics could not be finalized",
                    &error,
                    Some(&log_path),
                );
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            diagnostics::record(
                Diagnostic::error("build.failed", "Build failed")
                    .reason(format_error_chain(&error))
                    .suggestion("Review the error chain and earlier diagnostics before retrying"),
            );
            let finish_error = diagnostics::finish().err();
            print_error("Build failed", &error, Some(&log_path));
            if let Some(finish_error) = finish_error {
                eprintln!("  Diagnostics could not be finalized: {finish_error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn command_line_failure(error: clap::Error) -> ExitCode {
    let message = error.to_string();
    let _ = error.print();

    match diagnostics::init() {
        Ok(_) => {
            diagnostics::record(
                Diagnostic::error("cli.invalid-arguments", "Invalid command-line arguments")
                    .reason(message.trim())
                    .suggestion("Run ygo-cards --help to review the supported options"),
            );
            match diagnostics::finish() {
                Ok(()) => eprintln!("Diagnostics: {BUILD_LOG_PATH}"),
                Err(error) => eprintln!("Could not finalize diagnostics: {error:#}"),
            }
        }
        Err(error) => eprintln!("Could not initialize diagnostics: {error:#}"),
    }
    ExitCode::from(2)
}

fn generate(options: Options) -> Result<()> {
    prepare_resources(options.refresh_resources)?;

    let generation_options = GenerationOptions {
        check_images: options.check_images,
        skip_image_failures: options.skip_image_failures,
    };
    let ot_report = cards::ot::generate(generation_options)?;
    print_dataset_report(&ot_report);

    let rd_report = cards::rd::generate(generation_options)?;
    print_dataset_report(&rd_report);

    let reports = [&ot_report, &rd_report];
    let latest_comparison = compare_latest_release(&reports)?;
    let diagnostic_snapshot = diagnostics::snapshot()?;
    anyhow::ensure!(
        diagnostic_snapshot.errors() == 0,
        "build recorded {} internal error diagnostics; see {BUILD_LOG_PATH}",
        diagnostic_snapshot.errors()
    );
    write_summary_report(
        &reports,
        &latest_comparison,
        &diagnostic_snapshot,
        Path::new(SUMMARY_REPORT),
    )?;
    print_summary_report(&reports, &latest_comparison.datasets);

    Ok(())
}

fn prepare_resources(refresh: bool) -> Result<()> {
    if !refresh {
        resources::ensure_all()?;
        println!("Resources\n  Ready          assets/");
        return Ok(());
    }

    println!("Resources");
    for resource in resources::download_all()? {
        println!(
            "  {:<28} {:>10} bytes, {} {}",
            resource.path.display(),
            format_count(resource.bytes),
            resource.attempts,
            if resource.attempts == 1 {
                "attempt"
            } else {
                "attempts"
            }
        );
    }
    Ok(())
}

fn print_error(title: &str, error: &anyhow::Error, log_path: Option<&Path>) {
    eprintln!("\n{title}");
    for (index, cause) in error.chain().enumerate() {
        if index == 0 {
            eprintln!("  Error: {cause}");
        } else if index == 1 {
            eprintln!("  Caused by:");
            eprintln!("    {index}. {cause}");
        } else {
            eprintln!("    {index}. {cause}");
        }
    }
    if let Some(path) = log_path {
        eprintln!("  Diagnostics: {}", path.display());
    }
}

fn format_error_chain(error: &anyhow::Error) -> String {
    let mut chain = error.chain();
    let mut output = chain
        .next()
        .map(ToString::to_string)
        .unwrap_or_else(|| String::from("unknown error"));
    for cause in chain {
        output.push_str("\ncaused by: ");
        output.push_str(&cause.to_string());
    }
    output
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn formats_error_chains_on_separate_lines() {
        let error = std::fs::read_to_string("missing-file")
            .context("failed to load fixture")
            .unwrap_err();

        let formatted = format_error_chain(&error);

        assert!(formatted.starts_with("failed to load fixture\ncaused by: "));
        assert!(formatted.contains("No such file or directory"));
    }
}
