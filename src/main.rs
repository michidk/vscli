#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic)]

//! A CLI tool to launch vscode projects, which supports devcontainer.

#[cfg(feature = "recent-ui")]
mod history;
mod launch;
mod opts;
#[cfg(feature = "recent-ui")]
mod ui;
mod workspace;

#[cfg(feature = "recent-ui")]
use chrono::Utc;
use clap::Parser;
use color_eyre::eyre::Result;
use log::debug;
use std::io::Write;

#[cfg(feature = "recent-ui")]
use crate::history::{Entry, Tracker};

use crate::{
    launch::{Behavior, Config},
    opts::Opts,
    workspace::Workspace,
};

/// Entry point for `vscli`.
fn main() -> Result<()> {
    color_eyre::install()?;

    let opts = Opts::parse();

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(opts.verbosity.as_str()),
    )
    .format(log_format)
    .init();

    debug!("Parsed Opts:\n{:#?}", opts);

    let mut tracker_path = dirs::data_local_dir().expect("Local data dir not found.");
    tracker_path.push("vscli");
    tracker_path.push(".vscli_history.json");

    #[cfg(feature = "recent-ui")]
    let mut tracker = Tracker::load(&tracker_path)?;

    match &opts.command {
        #[cfg(feature = "recent-ui")]
        Some(opts::Commands::Recent) => {
            let res = ui::start(&mut tracker)?;
            if let Some(entry) = res {
                let ws = Workspace::from_path(&entry.path)?;
                let name = ws.workspace_name.clone();
                let lc = Config::new(ws, entry.behavior.clone(), opts.dry_run);
                lc.launch()?;

                tracker.push(Entry {
                    name,
                    path: entry.path.clone(),
                    last_opened: Utc::now(),
                    behavior: entry.behavior.clone(),
                });
            }
        }
        None => {
            let path = opts.path.as_path();
            let ws = Workspace::from_path(path)?;
            #[cfg(feature = "recent-ui")]
            let name = ws.workspace_name.clone();

            let behavior = Behavior {
                strategy: opts.behavior,
                insiders: opts.insiders,
                args: opts.args,
            };
            let lc = Config::new(ws, behavior.clone(), opts.dry_run);
            lc.launch()?;

            #[cfg(feature = "recent-ui")]
            tracker.push(Entry {
                name,
                path: path.canonicalize()?,
                last_opened: Utc::now(),
                behavior,
            });
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }

    #[cfg(feature = "recent-ui")]
    tracker.store()?;

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
