#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic)]

//! A CLI tool to launch vscode projects, which supports dev container.

mod config_store;
mod container;
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
use std::path::PathBuf;

use crate::config_store::ConfigStore;
use crate::history::{Entry, Tracker};

use crate::{
    launch::{Behavior, Setup},
    opts::Opts,
    workspace::Workspace,
};

fn load_tracker(history_path: Option<PathBuf>) -> Result<Tracker> {
    let path = history_path.unwrap_or_else(|| {
        let mut p = dirs::data_local_dir().expect("Local data dir not found.");
        p.push("vscli");
        p.push("history.json");
        p
    });
    Tracker::load(path)
}

fn resolve_launch_config(config: Option<&PathBuf>, store: &ConfigStore) -> Result<Option<PathBuf>> {
    config
        .map(|c| {
            store
                .resolve(c)
                .ok_or_else(|| color_eyre::eyre::eyre!("Config not found: {}", c.display()))
        })
        .transpose()
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let opts = Opts::parse();
    let opts_dbg = format!("{opts:#?}");

    env_logger::Builder::from_default_env()
        .filter_level(opts.verbose.log_level_filter())
        .format(move |buf, record| log_format(buf, record, opts.verbose.log_level_filter()))
        .init();

    trace!("Parsed Opts:\n{opts_dbg}");

    let config_store = ConfigStore::new(opts.config_dir);

    match opts.command {
        opts::Commands::Open { path, launch } => {
            let mut tracker = load_tracker(opts.history_path)?;
            let path = path.as_path();
            let ws = Workspace::from_path(path)?;
            let ws_name = ws.name.clone();

            let resolved_config = resolve_launch_config(launch.config.as_ref(), &config_store)?;
            let config_name = resolved_config
                .as_ref()
                .and_then(|p| config_store::config_name_from_path(p, &config_store));

            let behavior = Behavior {
                strategy: launch.behavior.unwrap_or_default(),
                args: launch.args,
                command: launch.command.unwrap_or_else(|| "code".to_string()),
            };
            let setup = Setup::new(ws, behavior.clone(), opts.dry_run);
            let dev_container = setup.launch(resolved_config)?;

            tracker.history.upsert(Entry {
                workspace_name: ws_name,
                dev_container_name: dev_container.as_ref().and_then(|dc| dc.name.clone()),
                config_name,
                workspace_path: path.canonicalize()?,
                config_path: dev_container.map(|dc| dc.config_path),
                behavior,
                last_opened: Utc::now(),
            });
            tracker.store()?;
        }
        opts::Commands::Recent {
            launch,
            hide_instructions,
            hide_info,
        } => {
            let mut tracker = load_tracker(opts.history_path)?;
            let res = ui::start(&mut tracker, hide_instructions, hide_info)?;
            if let Some((id, mut entry)) = res {
                let ws = Workspace::from_path(&entry.workspace_path)?;
                let ws_name = ws.name.clone();

                if let Some(cmd) = launch.command {
                    entry.behavior.command = cmd;
                }
                if let Some(beh) = launch.behavior {
                    entry.behavior.strategy = beh;
                }
                if !launch.args.is_empty() {
                    entry.behavior.args = launch.args;
                }

                let resolved_config = if launch.config.is_some() {
                    resolve_launch_config(launch.config.as_ref(), &config_store)?
                } else {
                    entry.config_path.clone()
                };

                let config_name = resolved_config
                    .as_ref()
                    .and_then(|p| config_store::config_name_from_path(p, &config_store));

                let setup = Setup::new(ws, entry.behavior.clone(), opts.dry_run);
                let dev_container = setup.launch(resolved_config)?;

                tracker.history.update(
                    id,
                    Entry {
                        workspace_name: ws_name,
                        dev_container_name: dev_container.as_ref().and_then(|dc| dc.name.clone()),
                        config_name,
                        workspace_path: entry.workspace_path.clone(),
                        config_path: dev_container.map(|dc| dc.config_path),
                        behavior: entry.behavior.clone(),
                        last_opened: Utc::now(),
                    },
                );
            }
            tracker.store()?;
        }
        opts::Commands::Config { action } => {
            let editor = std::env::var("VSCLI_EDITOR").unwrap_or_else(|_| "code".to_string());
            config_store::run_command(action, &config_store, &editor)?;
        }
        opts::Commands::Container { action } => {
            let editor = std::env::var("VSCLI_EDITOR").unwrap_or_else(|_| "code".to_string());
            container::run_command(action, &editor)?;
        }
    }

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
