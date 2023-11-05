#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic)]

//! A CLI tool to launch vscode projects, which supports dev container.

mod history;
mod launch;
mod opts;
mod ui;
mod uri;
mod workspace;

use chrono::Utc;
use clap::Parser;
use color_eyre::eyre::Result;
use log::trace;
use std::io::Write;

use crate::history::{Entry, Tracker};

use crate::{
    launch::{Behavior, Setup},
    opts::Opts,
    workspace::Workspace,
};

/// Entry point for `vscli`.
fn main() -> Result<()> {
    color_eyre::install()?;

    let opts = Opts::parse();
    let opts_dbg = format!("{opts:#?}");

    env_logger::Builder::from_default_env()
        .filter_level(opts.verbose.log_level_filter())
        .format(move |buf, record| log_format(buf, record, opts.verbose.log_level_filter()))
        .init();

    trace!("Parsed Opts:\n{}", opts_dbg);

    // Setup the tracker
    let mut tracker = {
        let tracker_path = if let Some(path) = opts.history_path {
            path
        } else {
            let mut tracker_path = dirs::data_local_dir().expect("Local data dir not found.");
            tracker_path.push("vscli");
            tracker_path.push(".history.json");
            tracker_path
        };
        Tracker::load(tracker_path)?
    };

    match opts.command {
        opts::Commands::Open {
            path,
            args,
            behavior,
            index,
            config,
            insiders,
        } => {
            // get workspace from args
            let path = path.as_path();
            let ws = Workspace::from_path(path)?;
            let name = ws.name.clone();

            let behavior = Behavior {
                strategy: behavior,
                insiders,
                args: args.clone(),
            };
            let lc = Setup::new(ws, behavior.clone(), opts.dry_run, index, config);
            lc.launch()?;

            tracker.push(Entry {
                name,
                path: path.canonicalize()?,
                last_opened: Utc::now(),
                behavior,
            });
        }
        opts::Commands::Recent => {
            // get workspace from user selection
            let res = ui::start(&mut tracker)?;
            if let Some(entry) = res {
                let ws = Workspace::from_path(&entry.path)?;
                let _name = ws.name.clone();
                // TODO: store dev container path in entry
                // let lc = LaunchConfig::new(ws, entry.behavior.clone(), None, opts.dry_run, *index, *config);
                // lc.launch()?;

                // tracker.push(Entry {
                //     name,
                //     path: entry.path.clone(),
                //     last_opened: Utc::now(),
                //     behavior: entry.behavior.clone(),
                // });
            }
        }
    }

    tracker.store()?;

    Ok(())
}

/// Formats the log messages in a minimalistic way, since we don't have a lot of output.
fn log_format(
    buf: &mut env_logger::fmt::Formatter,
    record: &log::Record,
    filter: log::LevelFilter,
) -> std::io::Result<()> {
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

    // Default behavior (for info messages): only print message
    // but if level is not info and filter is set, prefix it with the colored level
    if level == log::Level::Info && filter == log::LevelFilter::Info {
        writeln!(buf, "{}", record.args())
    } else {
        writeln!(buf, "{}: {}", colored_level, record.args())
    }
}
