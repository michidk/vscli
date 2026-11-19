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
use color_eyre::eyre::{Result, WrapErr, bail};
use log::trace;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config_store::ConfigStore;
use crate::history::{Entry, Tracker};

use crate::{
    launch::{Behavior, ContainerStrategy, Setup},
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
    if path_abs.starts_with(&root) {
        let sub = if path_abs == root {
            None
        } else {
            path_abs.strip_prefix(&root).ok().map(Path::to_path_buf)
        };
        Ok((root, sub))
    } else {
        Ok((path_abs, None))
    }
}

fn resolve_strategy_for_remote(
    remote_host: Option<&str>,
    strategy: Option<ContainerStrategy>,
) -> Result<ContainerStrategy> {
    if remote_host.is_some() {
        match strategy {
            None | Some(ContainerStrategy::ForceClassic) => Ok(ContainerStrategy::ForceClassic),
            Some(ContainerStrategy::Detect) => {
                bail!("--behavior detect is not supported with --remote-host.")
            }
            Some(ContainerStrategy::ForceContainer) => {
                bail!("--behavior force-container is not supported with --remote-host.")
            }
        }
    } else {
        Ok(strategy.unwrap_or_default())
    }
}

fn normalize_recent_strategy_for_remote(
    remote_host: Option<&str>,
    cli_strategy: Option<ContainerStrategy>,
    stored_strategy: ContainerStrategy,
) -> Result<ContainerStrategy> {
    if remote_host.is_none() {
        return Ok(cli_strategy.unwrap_or(stored_strategy));
    }

    match cli_strategy {
        Some(strategy) => resolve_strategy_for_remote(remote_host, Some(strategy)),
        None => Ok(ContainerStrategy::ForceClassic),
    }
}

fn ensure_remote_has_no_config(remote_host: Option<&str>, config: Option<&Path>) -> Result<()> {
    if remote_host.is_some() && config.is_some() {
        bail!(
            "--config cannot be combined with --remote-host; point vscli at the remote workspace path instead."
        );
    }

    Ok(())
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
        ensure_remote_has_no_config(launch.remote_host.as_deref(), launch.config.as_deref())?;
        let resolved_config = resolve_launch_config(launch.config.as_ref(), &self.config_store)?;
        let config_name = resolved_config
            .as_ref()
            .and_then(|config| config_store::config_name_from_path(config, &self.config_store));
        let (workspace_path, subfolder) = if let Some(config) = resolved_config.as_ref() {
            workspace_root_from_config(config, &path)?
        } else {
            (path, None)
        };

        let workspace = if let Some(remote_host) = launch.remote_host {
            Workspace::from_remote_path(&workspace_path, remote_host)?
        } else {
            Workspace::from_path(&workspace_path)?
        };
        let tracked_workspace_path = workspace.path.clone();
        let remote_host = workspace.remote_host.clone();
        let workspace_name = workspace.name.clone();
        let behavior = Behavior {
            strategy: resolve_strategy_for_remote(remote_host.as_deref(), launch.behavior)?,
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
            workspace_path: tracked_workspace_path,
            remote_host,
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

        let remote_host = launch.remote_host.clone().or(entry.remote_host.clone());
        ensure_remote_has_no_config(remote_host.as_deref(), launch.config.as_deref())?;
        let workspace = if let Some(remote_host) = remote_host.clone() {
            Workspace::from_remote_path(&entry.workspace_path, remote_host)?
        } else {
            Workspace::from_path(&entry.workspace_path)?
        };
        let workspace_name = workspace.name.clone();
        if let Some(command) = launch.command {
            entry.behavior.command = command;
        }
        entry.behavior.strategy = normalize_recent_strategy_for_remote(
            remote_host.as_deref(),
            launch.behavior,
            entry.behavior.strategy,
        )?;
        if !launch.args.is_empty() {
            entry.behavior.args = launch.args;
        }

        let resolved_config = if remote_host.is_some() {
            None
        } else if launch.config.is_some() {
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
                remote_host,
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

#[cfg(test)]
mod tests {
    use super::{
        normalize_recent_strategy_for_remote, resolve_strategy_for_remote,
        workspace_root_from_config,
    };
    use crate::launch::ContainerStrategy;
    use std::path::{Path, PathBuf};

    fn unique_test_dir(name: &str) -> PathBuf {
        let unique = format!(
            "vscli-main-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn preserves_project_path_for_external_config() {
        let root = unique_test_dir("external-config");
        let config = root
            .join("configs")
            .join("rust-dev")
            .join(".devcontainer")
            .join("devcontainer.json");
        let project = root.join("projects").join("my-app");

        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(&config, "{}\n").unwrap();

        let (workspace, subfolder) = workspace_root_from_config(&config, &project).unwrap();

        assert_eq!(workspace, project.canonicalize().unwrap());
        assert_eq!(subfolder, None);
    }

    #[test]
    fn derives_subfolder_when_path_is_inside_config_workspace() {
        let root = unique_test_dir("subfolder");
        let workspace = root.join("workspace");
        let config = workspace.join(".devcontainer").join("devcontainer.json");
        let project = workspace.join("packages").join("api");

        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(&config, "{}\n").unwrap();

        let (resolved_workspace, subfolder) =
            workspace_root_from_config(&config, &project).unwrap();

        assert_eq!(resolved_workspace, workspace.canonicalize().unwrap());
        assert_eq!(subfolder.as_deref(), Some(Path::new("packages/api")));
    }

    #[test]
    fn remote_workspaces_default_to_force_classic() {
        let strategy = resolve_strategy_for_remote(Some("remote-test"), None).unwrap();
        assert_eq!(strategy, ContainerStrategy::ForceClassic);
    }

    #[test]
    fn remote_workspaces_reject_detect_behavior() {
        let err = resolve_strategy_for_remote(Some("remote-test"), Some(ContainerStrategy::Detect))
            .unwrap_err();
        assert!(err.to_string().contains("not supported with --remote-host"));
    }

    #[test]
    fn remote_workspaces_reject_force_container_behavior() {
        let err = resolve_strategy_for_remote(
            Some("remote-test"),
            Some(ContainerStrategy::ForceContainer),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not supported with --remote-host"));
    }

    #[test]
    fn recent_remote_reopen_coerces_stored_detect_to_force_classic() {
        let strategy = normalize_recent_strategy_for_remote(
            Some("remote-test"),
            None,
            ContainerStrategy::Detect,
        )
        .unwrap();
        assert_eq!(strategy, ContainerStrategy::ForceClassic);
    }
}
