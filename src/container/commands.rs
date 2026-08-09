use super::{info, list, stop};
use crate::opts::ContainerAction;
use crate::ui;
use crate::workspace::{self, DevContainer, Workspace};
use color_eyre::eyre::{Result, bail};
use log::info;
use std::path::{Path, PathBuf};

fn run_ui(editor: &str) -> Result<()> {
    let containers = list(false)?;
    if containers.is_empty() {
        println!("no running devcontainers");
        return Ok(());
    }

    let mut stop_cb = |item: &ui::ContainerItem| {
        if let Err(error) = stop(&item.0.id) {
            log::warn!("Failed to stop container {}: {error}", item.0.short_id);
        }
    };
    let selected = ui::pick_container(containers, ui::PickerOpts::default(), Some(&mut stop_cb))?;
    let Some(container) = selected else {
        return Ok(());
    };

    info!("Reopening container {} ...", container.short_id);
    let container_info = info(&container.id)?;
    let local_folder = workspace::resolve_local_path(&container_info.local_folder);
    let project_path = Path::new(&local_folder);
    if !project_path.exists() {
        bail!("Project path does not exist: {}", project_path.display());
    }

    let workspace = Workspace::from_path(project_path)?;
    let config_file = workspace::resolve_local_path(&container_info.config_file);
    let config_path = PathBuf::from(config_file);
    if config_path.exists() {
        let dev_container = DevContainer::from_config(&config_path, &workspace.name)?;
        workspace.open(vec![], false, &dev_container, editor, None)
    } else {
        workspace.open_classic(vec![], false, editor)
    }
}

fn print_containers(all: bool) -> Result<()> {
    let containers = list(all)?;
    if containers.is_empty() {
        println!("no {}devcontainers", if all { "" } else { "running " });
        return Ok(());
    }

    let id_width = 12;
    let status_width = containers
        .iter()
        .map(|container| container.status.len())
        .max()
        .unwrap_or(6);
    let image_width = containers
        .iter()
        .map(|container| container.image.len())
        .max()
        .unwrap_or(5);
    println!(
        "{:<id_width$}  {:<status_width$}  {:<image_width$}  PROJECT PATH",
        "CONTAINER ID", "STATUS", "IMAGE"
    );
    for container in containers {
        println!(
            "{:<id_width$}  {:<status_width$}  {:<image_width$}  {}",
            container.short_id, container.status, container.image, container.local_folder
        );
    }
    Ok(())
}

fn print_container_info(id: &str) -> Result<()> {
    let container = info(id)?;
    let created = chrono::DateTime::parse_from_rfc3339(&container.created)
        .map(|date| {
            chrono::DateTime::<chrono::Local>::from(date)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or(container.created);
    println!("Container:    {}", container.id);
    println!("Name:         {}", container.name);
    println!("Image:        {}", container.image);
    println!("Status:       {}", container.status);
    println!("Created:      {created}");
    println!("Project:      {}", container.local_folder);
    println!("Config:       {}", container.config_file);
    println!("Ports:        {}", container.ports);
    if container.mounts.is_empty() {
        println!("Mounts:       none");
    } else {
        for (index, mount) in container.mounts.iter().enumerate() {
            if index == 0 {
                println!("Mounts:       {mount}");
            } else {
                println!("              {mount}");
            }
        }
    }
    Ok(())
}

pub fn run_command(action: ContainerAction, editor: &str) -> Result<()> {
    match action {
        ContainerAction::Ui => run_ui(editor)?,
        ContainerAction::List { all } => print_containers(all)?,
        ContainerAction::Info { id } => print_container_info(&id)?,
        ContainerAction::Stop { id } => {
            stop(&id)?;
            info!("Stopped container {id}");
        }
    }
    Ok(())
}
