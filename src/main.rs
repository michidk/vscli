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
use color_eyre::eyre::{Result, WrapErr};
use log::trace;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config_store::ConfigStore;
use crate::history::{Entry, Tracker};

use crate::{
    launch::{Behavior, Setup},
    opts::{Commands, LaunchArgs, Opts},
    ui::PickerOpts,
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

fn workspace_root_from_config(
    config: &Path,
    path_arg: &Path,
) -> Result<(PathBuf, Option<PathBuf>)> {
    let abs = std::fs::canonicalize(config)
        .wrap_err_with(|| format!("Config path does not exist: {}", config.display()))?;
    let mut current = abs.as_path();
    let root = loop {
        let Some(parent) = current.parent() else {
            break abs.parent().unwrap_or(&abs).to_path_buf();
        };
        if parent.file_name().is_some_and(|n| n == ".devcontainer") {
            break parent.parent().unwrap_or(parent).to_path_buf();
        }
        current = parent;
    };
    let path_abs = std::fs::canonicalize(path_arg).unwrap_or(path_arg.to_path_buf());
    let sub = if path_abs.starts_with(&root) && path_abs != root {
        path_abs.strip_prefix(&root).ok().map(Path::to_path_buf)
    } else {
        None
    };
    Ok((root, sub))
}

struct Application {
    history_path: Option<PathBuf>,
    config_store: ConfigStore,
    dry_run: bool,
}

impl Application {
    fn run(&self, command: Commands) -> Result<()> {
        match command {
            Commands::Open { path, launch } => self.open(path, launch),
            Commands::Recent {
                launch,
                hide_instructions,
                hide_info,
            } => self.open_recent(
                launch,
                PickerOpts {
                    hide_instructions,
                    hide_info,
                },
            ),
            Commands::Config { action } => {
                let editor = std::env::var("VSCLI_EDITOR").unwrap_or_else(|_| "code".to_string());
                config_store::run_command(action, &self.config_store, &editor)
            }
            Commands::Container { action } => {
                let editor = std::env::var("VSCLI_EDITOR").unwrap_or_else(|_| "code".to_string());
                container::run_command(action, &editor)
            }
        }
    }

    fn open(&self, path: PathBuf, launch: LaunchArgs) -> Result<()> {
        let mut tracker = load_tracker(self.history_path.clone())?;
        let resolved_config = resolve_launch_config(launch.config.as_ref(), &self.config_store)?;
        let config_name = resolved_config
            .as_ref()
            .and_then(|config| config_store::config_name_from_path(config, &self.config_store));
        let (workspace_path, subfolder) = if let Some(config) = resolved_config.as_ref() {
            workspace_root_from_config(config, &path)?
        } else {
            (path, None)
        };

        let workspace = Workspace::from_path(&workspace_path)?;
        let workspace_name = workspace.name.clone();
        let behavior = Behavior {
            strategy: launch.behavior.unwrap_or_default(),
            args: launch.args,
            command: launch.command.unwrap_or_else(|| "code".to_string()),
        };
        let setup = Setup::new(workspace, behavior.clone(), self.dry_run);
        let dev_container = setup.launch(resolved_config, subfolder.as_deref())?;

        tracker.history.upsert(Entry {
            workspace_name,
            dev_container_name: dev_container
                .as_ref()
                .and_then(|container| container.name.clone()),
            config_name,
            workspace_path: workspace_path.canonicalize()?,
            config_path: dev_container.map(|container| container.config_path),
            behavior,
            last_opened: Utc::now(),
        });
        tracker.store()
    }

    fn open_recent(&self, launch: LaunchArgs, picker_opts: PickerOpts) -> Result<()> {
        let mut tracker = load_tracker(self.history_path.clone())?;
        let selected = ui::start(
            &mut tracker,
            picker_opts.hide_instructions,
            picker_opts.hide_info,
        )?;
        let Some((id, mut entry)) = selected else {
            return tracker.store();
        };

        let workspace = Workspace::from_path(&entry.workspace_path)?;
        let workspace_name = workspace.name.clone();
        if let Some(command) = launch.command {
            entry.behavior.command = command;
        }
        if let Some(strategy) = launch.behavior {
            entry.behavior.strategy = strategy;
        }
        if !launch.args.is_empty() {
            entry.behavior.args = launch.args;
        }

        let resolved_config = if launch.config.is_some() {
            resolve_launch_config(launch.config.as_ref(), &self.config_store)?
        } else {
            entry.config_path.clone()
        };
        let config_name = resolved_config
            .as_ref()
            .and_then(|config| config_store::config_name_from_path(config, &self.config_store));
        let setup = Setup::new(workspace, entry.behavior.clone(), self.dry_run);
        let dev_container = setup.launch(resolved_config, None)?;

        tracker.history.update(
            id,
            Entry {
                workspace_name,
                dev_container_name: dev_container
                    .as_ref()
                    .and_then(|container| container.name.clone()),
                config_name,
                workspace_path: entry.workspace_path,
                config_path: dev_container.map(|container| container.config_path),
                behavior: entry.behavior,
                last_opened: Utc::now(),
            },
        );
        tracker.store()
    }
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

    Application {
        history_path: opts.history_path,
        config_store: ConfigStore::new(opts.config_dir),
        dry_run: opts.dry_run,
    }
    .run(opts.command)
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
