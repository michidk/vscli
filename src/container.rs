use bollard::Docker;
use bollard::query_parameters::{InspectContainerOptionsBuilder, ListContainersOptionsBuilder};
use color_eyre::eyre::{Result, WrapErr};
use log::debug;
use std::collections::HashMap;

mod commands;

pub use commands::run_command;

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
