#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic)]

//! A CLI tool to launch vscode projects, which supports devcontainers.

mod launch;
mod opts;
mod workspace;

use color_eyre::eyre::Result;
use log::debug;
use std::io::Write;
use structopt::StructOpt;

use crate::{launch::Config, opts::Opts, workspace::Workspace};

/// Entry point for `vscli`.
fn main() -> Result<()> {
    color_eyre::install()?;

    let opts = Opts::from_args();

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(opts.verbosity.as_str()),
    )
    .format(log_format)
    .init();

    debug!("Parsed Opts:\n{:#?}", opts);

    let ws = Workspace::from_path(opts.path.as_path())?;
    let lc = Config::new(ws, opts.behaviour, opts.insiders, opts.args);
    lc.launch()?;

    Ok(())
}

/// Formats the log messages in a minimalistic way, since we don't have a lot of output.
fn log_format(buf: &mut env_logger::fmt::Formatter, record: &log::Record) -> std::io::Result<()> {
    let level = record.level();
    let level_char = match level {
        log::Level::Trace => 'T',
        log::Level::Debug => 'D',
        log::Level::Info => 'I',
        log::Level::Warn => 'W',
        log::Level::Error => 'E',
    };
    // color using shell escape codes
    let colored_level = match level {
        log::Level::Trace => format!("\x1b[37m{level_char}\x1b[0m"),
        log::Level::Debug => format!("\x1b[36m{level_char}\x1b[0m"),
        log::Level::Info => format!("\x1b[32m{level_char}\x1b[0m"),
        log::Level::Warn => format!("\x1b[33m{level_char}\x1b[0m"),
        log::Level::Error => format!("\x1b[31m{level_char}\x1b[0m"),
    };

    writeln!(buf, "{}: {}", colored_level, record.args())
}
