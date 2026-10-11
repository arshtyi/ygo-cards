use std::{
    backtrace::{Backtrace, BacktraceStatus},
    fs,
    path::Path,
    process::ExitCode,
};

use anyhow::{Context, Result};
use clap::error::ErrorKind;

use crate::{
    cards::{self, GenerationOptions},
    cli::Options,
    diagnostics::{self, Diagnostic},
    formatting::format_count,
    latest::compare_latest_release,
    report::{self, ReportScope, SUMMARY_REPORT},
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

    install_panic_hook();

    match generate(options) {
        Ok(()) => {
            println!("Report: {SUMMARY_REPORT}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            diagnostics::record(
                Diagnostic::error("build.failed", "Build failed")
                    .reason(format_error_chain(&error))
                    .suggestion("Review the error chain and earlier diagnostics before retrying"),
            );
            eprintln!("Build failed: {error}");
            save_failure_report();
            ExitCode::FAILURE
        }
    }
}

fn command_line_failure(error: clap::Error) -> ExitCode {
    let message = error.to_string();
    let _ = error.print();

    diagnostics::record(
        Diagnostic::error("cli.invalid-arguments", "Invalid command-line arguments")
            .reason(message.trim())
            .suggestion("Run ygo-cards --help to review the supported options"),
    );
    save_failure_report();
    ExitCode::from(2)
}

fn generate(options: Options) -> Result<()> {
    // Partial or failed runs must not retain artifacts from a previous full comparison.
    for filename in ["output/comparison.json", "output/ot.diff", "output/rd.diff"] {
        match fs::remove_file(filename) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to remove {filename}"));
            }
        }
    }
    if let Some(limit) = options.limit {
        println!(
            "Limited test run: up to {limit} input cards per dataset; release comparison skipped."
        );
    }
    prepare_resources(options.refresh_resources)?;

    let generation_options = GenerationOptions {
        limit: options.limit,
        check_images: options.check_images,
        skip_image_failures: options.skip_image_failures,
    };
    println!("Generating OT cards...");
    let ot_report = cards::ot::generate(generation_options)?;

    println!("Generating RD cards...");
    let rd_report = cards::rd::generate(generation_options)?;

    let reports = [&ot_report, &rd_report];
    let diagnostic_snapshot = diagnostics::snapshot()?;
    anyhow::ensure!(
        diagnostic_snapshot.errors() == 0,
        "build recorded {} internal error diagnostics; see {SUMMARY_REPORT}",
        diagnostic_snapshot.errors()
    );
    if let Some(limit) = options.limit {
        report::write_report(
            &reports,
            ReportScope::Limited(limit.get()),
            &diagnostic_snapshot,
            Path::new(SUMMARY_REPORT),
        )?;
    } else {
        println!("Comparing with the latest release...");
        let comparison = compare_latest_release(&reports)?;
        report::write_report(
            &reports,
            ReportScope::Full(&comparison),
            &diagnostic_snapshot,
            Path::new(SUMMARY_REPORT),
        )?;
        comparison.write_artifacts(Path::new("output"))?;
    }
    println!(
        "Generated {} OT and {} RD cards. Warnings: {}. Errors: {}.",
        format_count(ot_report.cards_written),
        format_count(rd_report.cards_written),
        diagnostic_snapshot.warnings(),
        diagnostic_snapshot.errors(),
    );

    Ok(())
}

fn prepare_resources(refresh: bool) -> Result<()> {
    if refresh {
        println!("Refreshing upstream resources...");
    }
    resources::prepare(refresh)?;
    println!("Resources ready: assets/");
    Ok(())
}

fn save_failure_report() {
    match diagnostics::snapshot()
        .and_then(|snapshot| report::write_failure_report(&snapshot, Path::new(SUMMARY_REPORT)))
    {
        Ok(()) => eprintln!("Report: {SUMMARY_REPORT}"),
        Err(error) => eprintln!("Could not write failure report: {error:#}"),
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(String::as_str)
            })
            .unwrap_or("panic payload was not a string");
        let mut diagnostic =
            Diagnostic::error("runtime.panic", "Unexpected internal error").reason(message);
        if let Some(location) = panic_info.location() {
            diagnostic = diagnostic.context("Location", location);
        }
        let backtrace = Backtrace::capture();
        if backtrace.status() == BacktraceStatus::Captured {
            diagnostic = diagnostic
                .context("Backtrace", &backtrace)
                .suggestion("Include the report when reporting this internal error");
        } else {
            diagnostic = diagnostic.suggestion(
                "Re-run with RUST_BACKTRACE=1 and include the report when reporting it",
            );
        }
        diagnostics::record(diagnostic);
        eprintln!("Unexpected internal error: {message}");
        save_failure_report();
    }));
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
    use std::process::Command;

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

    #[test]
    fn limited_generation_counts_skips_and_never_compares_releases() {
        const LIMIT_CHILD: &str = "YGO_CARDS_TEST_LIMIT_CHILD";
        if let Ok(limit) = std::env::var(LIMIT_CHILD) {
            generate(Options {
                limit: Some(limit.parse().unwrap()),
                ..Options::default()
            })
            .unwrap();
            return;
        }

        let directory = std::env::temp_dir().join(format!("ygo-limit-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        for (name, contents) in [
            ("ot/lflist.conf", "!OCG\n!TCG\n"),
            (
                "ot/genesys.conf",
                "# Genesys points\n# Disable Pendulum and Link monsters\n",
            ),
            ("rd/lflist.conf", "# forbidden\n"),
        ] {
            let path = directory.join("assets").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        for (name, last_id, invalid_id) in [("ot/cards.cdb", 3, 1), ("rd/rd_standard.cdb", 7, 5)] {
            let connection =
                rusqlite::Connection::open(directory.join("assets").join(name)).unwrap();
            connection.execute_batch("CREATE TABLE datas (id INTEGER PRIMARY KEY, alias INTEGER, type INTEGER, race INTEGER, attribute INTEGER, atk INTEGER, def INTEGER, level INTEGER);
                CREATE TABLE texts (id INTEGER PRIMARY KEY, name TEXT, desc TEXT);").unwrap();
            for id in (1..=last_id).rev() {
                connection
                    .execute("INSERT INTO datas VALUES (?1,0,2,0,0,0,0,0)", [id])
                    .unwrap();
                let name = if id == invalid_id {
                    String::new()
                } else {
                    format!("Card {id}")
                };
                connection
                    .execute(
                        "INSERT INTO texts VALUES (?1,?2,'Description')",
                        rusqlite::params![id, name],
                    )
                    .unwrap();
            }
        }
        fs::create_dir(directory.join("output")).unwrap();
        for limit in [2, 100] {
            for filename in ["comparison.json", "ot.diff", "rd.diff"] {
                fs::write(directory.join("output").join(filename), "stale").unwrap();
            }
            let result = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "app::tests::limited_generation_counts_skips_and_never_compares_releases",
                    "--nocapture",
                ])
                .env(LIMIT_CHILD, limit.to_string())
                // Accidental release requests must fail; this fixture needs no network.
                .env("HTTPS_PROXY", "http://127.0.0.1:1")
                .env("NO_PROXY", "")
                .current_dir(&directory)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            for (name, first_id) in [("ot.json", 2), ("rd.json", 6)] {
                let cards: Vec<serde_json::Value> = serde_json::from_str(
                    &fs::read_to_string(directory.join("output").join(name)).unwrap(),
                )
                .unwrap();
                assert_eq!(cards.len(), if limit == 2 { 1 } else { 2 });
                assert_eq!(cards[0]["id"], first_id);
            }
            let report = fs::read_to_string(directory.join(SUMMARY_REPORT)).unwrap();
            assert!(report.contains("Limited test run:"));
            assert!(
                report.contains("<summary>Other diagnostics (warnings: 2, errors: 0)</summary>")
            );
            assert_eq!(report.matches("| Code | card.skipped |").count(), 2);
            for (environment, id) in [("OT", 1), ("RD", 5)] {
                assert!(report.contains(&format!("| Environment | {environment} |")));
                assert!(report.contains(&format!("| Card ID | {id} |")));
            }
            assert_eq!(
                report.matches("name is empty or whitespace-only").count(),
                2
            );
            assert!(report.contains("<details>"));
            assert!(!report.contains("<details open"));
            assert!(!report.contains("Previous release:"));
            for filename in ["comparison.json", "ot.diff", "rd.diff"] {
                assert!(!directory.join("output").join(filename).exists());
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn reports_panics_with_earlier_diagnostics() {
        const PANIC_CHILD: &str = "YGO_CARDS_TEST_PANIC_CHILD";
        if std::env::var_os(PANIC_CHILD).is_some() {
            install_panic_hook();
            diagnostics::record(
                Diagnostic::warning("test.warning", "Earlier warning").context("Card ID", 42),
            );
            panic!("panic report fixture");
        }

        // Run the hook in a child so it cannot affect other tests or their reports.
        let directory = std::env::temp_dir().join(format!("ygo-panic-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::reports_panics_with_earlier_diagnostics",
            ])
            .env(PANIC_CHILD, "1")
            .env("RUST_BACKTRACE", "1")
            .current_dir(&directory)
            .output()
            .unwrap();
        let report = fs::read_to_string(directory.join(SUMMARY_REPORT)).unwrap();
        fs::remove_dir_all(directory).unwrap();

        assert!(!output.status.success());
        assert!(report.contains("<summary>Other diagnostics (warnings: 1, errors: 1)</summary>"));
        assert!(report.contains("test.warning"));
        assert!(report.contains("| Card ID | 42 |"));
        assert!(report.contains("runtime.panic"));
        assert!(report.contains("panic report fixture"));
        assert!(report.contains("| Location |"));
        assert!(report.contains("| Backtrace |"));
        assert!(report.contains("<details>"));
        assert!(!report.contains("<details open"));
    }
}
