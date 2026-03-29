use bollard::Docker;
use bollard::query_parameters::{InspectContainerOptionsBuilder, ListContainersOptionsBuilder};
use color_eyre::eyre::{Result, WrapErr, bail};
use log::{debug, info};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::opts::ContainerAction;
use crate::ui;
use crate::workspace::{self, DevContainer, Workspace};

/// A running or stopped devcontainer discovered via Docker labels.
#[derive(Debug, Clone)]
pub struct Container {
    /// Full container ID.
    pub id: String,
    /// Short (12-char) container ID.
    pub short_id: String,
    /// Host project path from `devcontainer.local_folder` label.
    pub local_folder: String,
    /// Config file path from `devcontainer.config_file` label.
    pub config_file: String,
    /// Container status string (e.g. "Up 2 hours", "Exited (0) 1 day ago").
    pub status: String,
    /// Image used by the container.
    pub image: String,
}

/// Detailed information about a single devcontainer.
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    /// Full container ID.
    pub id: String,
    /// Host project path.
    pub local_folder: String,
    /// Config file path.
    pub config_file: String,
    /// Container status.
    pub status: String,
    /// Image used.
    pub image: String,
    /// Container name.
    pub name: String,
    /// Creation time.
    pub created: String,
    /// Port mappings.
    pub ports: String,
    /// Bind mounts.
    pub mounts: Vec<String>,
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("Failed to create tokio runtime")
}

fn connect() -> Result<Docker> {
    Docker::connect_with_socket_defaults()
        .wrap_err("Failed to connect to Docker. Is the Docker daemon running?")
}

/// Lists devcontainers by querying Docker for containers with `devcontainer.local_folder` labels.
pub fn list(all: bool) -> Result<Vec<Container>> {
    runtime().block_on(list_async(all))
}

async fn list_async(all: bool) -> Result<Vec<Container>> {
    let docker = connect()?;

    let mut filters = HashMap::new();
    filters.insert(
        "label".to_string(),
        vec!["devcontainer.local_folder".to_string()],
    );

    let options = ListContainersOptionsBuilder::default()
        .all(all)
        .filters(&filters)
        .build();

    let containers = docker
        .list_containers(Some(options))
        .await
        .wrap_err("Failed to list containers")?;

    let result: Vec<Container> = containers
        .into_iter()
        .filter_map(|c| {
            let id = c.id?;
            let short_id = id[..12.min(id.len())].to_string();
            let labels = c.labels.unwrap_or_default();
            Some(Container {
                short_id,
                id,
                local_folder: labels
                    .get("devcontainer.local_folder")
                    .cloned()
                    .unwrap_or_default(),
                config_file: labels
                    .get("devcontainer.config_file")
                    .cloned()
                    .unwrap_or_default(),
                status: c.status.unwrap_or_default(),
                image: c.image.unwrap_or_default(),
            })
        })
        .collect();

    debug!("Found {} devcontainers", result.len());
    Ok(result)
}

/// Returns detailed information about a specific devcontainer.
pub fn info(id: &str) -> Result<ContainerInfo> {
    runtime().block_on(info_async(id))
}

async fn info_async(id: &str) -> Result<ContainerInfo> {
    let docker = connect()?;

    let options = InspectContainerOptionsBuilder::default().build();

    let detail = docker
        .inspect_container(id, Some(options))
        .await
        .wrap_err_with(|| format!("Failed to inspect container '{id}'"))?;

    let config = detail.config.unwrap_or_default();
    let labels = config.labels.unwrap_or_default();
    let state = detail.state.unwrap_or_default();

    let mounts: Vec<String> = detail
        .mounts
        .unwrap_or_default()
        .iter()
        .filter_map(|m| {
            let source = m.source.as_deref()?;
            let dest = m.destination.as_deref()?;
            let mount_type = m.typ.as_ref().map_or("unknown", |t| t.as_ref());
            Some(format!("{source} -> {dest} ({mount_type})"))
        })
        .collect();

    let ports = detail
        .network_settings
        .and_then(|ns| ns.ports)
        .map_or_else(|| "none".to_string(), |p| format_ports(&p));

    Ok(ContainerInfo {
        id: detail.id.unwrap_or_default(),
        name: detail
            .name
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string(),
        image: config.image.unwrap_or_default().clone(),
        status: state.status.map_or_else(String::new, |s| s.to_string()),
        created: detail.created.unwrap_or_default(),
        ports,
        mounts,
        local_folder: labels
            .get("devcontainer.local_folder")
            .cloned()
            .unwrap_or_default(),
        config_file: labels
            .get("devcontainer.config_file")
            .cloned()
            .unwrap_or_default(),
    })
}

/// Stops a devcontainer by ID or ID prefix.
pub fn stop(id: &str) -> Result<()> {
    runtime().block_on(stop_async(id))
}

async fn stop_async(id: &str) -> Result<()> {
    let docker = connect()?;

    docker
        .stop_container(id, None::<bollard::query_parameters::StopContainerOptions>)
        .await
        .wrap_err_with(|| format!("Failed to stop container '{id}'"))?;

    Ok(())
}

fn format_ports(ports: &HashMap<String, Option<Vec<bollard::models::PortBinding>>>) -> String {
    let mut formatted = Vec::new();
    for (container_port, host_bindings) in ports {
        if let Some(bindings) = host_bindings {
            for binding in bindings {
                let host_port = binding.host_port.as_deref().unwrap_or("?");
                formatted.push(format!("{host_port}->{container_port}"));
            }
        }
    }

    if formatted.is_empty() {
        String::from("none")
    } else {
        formatted.join(", ")
    }
}

/// Runs a container subcommand.
pub fn run_command(action: ContainerAction, editor: &str) -> Result<()> {
    match action {
        ContainerAction::Ui => {
            let containers = list(false)?;
            if containers.is_empty() {
                println!("no running devcontainers");
                return Ok(());
            }
            let mut stop_cb = |item: &ui::ContainerItem| {
                if let Err(e) = stop(&item.0.id) {
                    log::warn!("Failed to stop container {}: {e}", item.0.short_id);
                }
            };
            let selected =
                ui::pick_container(containers, ui::PickerOpts::default(), Some(&mut stop_cb))?;
            if let Some(c) = selected {
                info!("Reopening container {} ...", c.short_id);
                let ci = info(&c.id)?;
                let local_folder = workspace::resolve_local_path(&ci.local_folder);
                let project_path = Path::new(&local_folder);
                if project_path.exists() {
                    let ws = Workspace::from_path(project_path)?;
                    let config_file = workspace::resolve_local_path(&ci.config_file);
                    let config_path = PathBuf::from(&config_file);
                    if config_path.exists() {
                        let dev_container = DevContainer::from_config(&config_path, &ws.name)?;
                        ws.open(vec![], false, &dev_container, editor, None)?;
                    } else {
                        ws.open_classic(vec![], false, editor)?;
                    }
                } else {
                    bail!("Project path does not exist: {}", project_path.display());
                }
            }
        }
        ContainerAction::List { all } => {
            let containers = list(all)?;
            if containers.is_empty() {
                println!("no {}devcontainers", if all { "" } else { "running " });
                return Ok(());
            }

            let id_w = 12;
            let status_w = containers.iter().map(|c| c.status.len()).max().unwrap_or(6);
            let image_w = containers.iter().map(|c| c.image.len()).max().unwrap_or(5);
            println!(
                "{:<id_w$}  {:<status_w$}  {:<image_w$}  PROJECT PATH",
                "CONTAINER ID", "STATUS", "IMAGE"
            );
            for c in &containers {
                println!(
                    "{:<id_w$}  {:<status_w$}  {:<image_w$}  {}",
                    c.short_id, c.status, c.image, c.local_folder
                );
            }
        }
        ContainerAction::Info { id } => {
            let ci = info(&id)?;
            let created = chrono::DateTime::parse_from_rfc3339(&ci.created)
                .map(|dt| {
                    chrono::DateTime::<chrono::Local>::from(dt)
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string()
                })
                .unwrap_or(ci.created);
            println!("Container:    {}", ci.id);
            println!("Name:         {}", ci.name);
            println!("Image:        {}", ci.image);
            println!("Status:       {}", ci.status);
            println!("Created:      {created}");
            println!("Project:      {}", ci.local_folder);
            println!("Config:       {}", ci.config_file);
            println!("Ports:        {}", ci.ports);
            if ci.mounts.is_empty() {
                println!("Mounts:       none");
            } else {
                for (i, mount) in ci.mounts.iter().enumerate() {
                    if i == 0 {
                        println!("Mounts:       {mount}");
                    } else {
                        println!("              {mount}");
                    }
                }
            }
        }
        ContainerAction::Stop { id } => {
            stop(&id)?;
            info!("Stopped container {id}");
        }
    }
    Ok(())
}
