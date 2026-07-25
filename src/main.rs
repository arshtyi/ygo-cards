mod app;
mod cli;
mod latest;
mod report;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::execute()
}
