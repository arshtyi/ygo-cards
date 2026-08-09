mod app;
mod cards;
mod cli;
mod config;
mod diagnostics;
mod endpoints;
mod environment;
mod formatting;
mod http;
mod json;
mod latest;
mod report;
mod resources;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::execute()
}
