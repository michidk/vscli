use super::Pickable;
use crate::history::{Entry, EntryId};
use chrono::{DateTime, Local};
use ratatui::layout::Constraint;
use std::borrow::Cow;

#[derive(Debug, Clone)]
pub struct HistoryItem {
    pub id: EntryId,
    pub entry: Entry,
}

impl Pickable for HistoryItem {
    fn title() -> &'static str {
        "Recent Workspaces"
    }

    fn headers() -> &'static [&'static str] {
        &[
            "Workspace",
            "Dev Container",
            "Config",
            "Path",
            "Last Opened",
        ]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.entry.workspace_name.clone(),
            self.entry
                .dev_container_name
                .as_deref()
                .unwrap_or("")
                .to_string(),
            self.entry.config_name.as_deref().unwrap_or("").to_string(),
            self.entry.workspace_path.to_string_lossy().to_string(),
            DateTime::<Local>::from(self.entry.last_opened)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ]
    }

    fn search_fields(&self) -> Vec<String> {
        vec![
            self.entry.workspace_name.clone(),
            self.entry.dev_container_name.clone().unwrap_or_default(),
            self.entry.config_name.clone().unwrap_or_default(),
            self.entry.workspace_path.to_string_lossy().to_string(),
        ]
    }

    fn status_lines(&self) -> Vec<String> {
        let args_count = self.entry.behavior.args.len();
        let args_joined = self
            .entry
            .behavior
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<Cow<'_, str>>>()
            .join(", ");
        let config_path = self
            .entry
            .config_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();

        vec![
            format!(
                "Strategy: {} • Command: {} • Args ({args_count}): {args_joined}",
                self.entry.behavior.strategy, self.entry.behavior.command,
            ),
            format!("Dev Container: {config_path}"),
        ]
    }

    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let workspace_width = max_widths.first().copied().unwrap_or(20).clamp(9, 60);
        let devcontainer_width = max_widths.get(1).copied().unwrap_or(20).clamp(9, 60);
        let config_width = max_widths.get(2).copied().unwrap_or(6).clamp(6, 40);

        vec![
            Constraint::Min(u16::try_from(workspace_width).unwrap_or(u16::MAX)),
            Constraint::Min(u16::try_from(devcontainer_width).unwrap_or(u16::MAX)),
            Constraint::Min(u16::try_from(config_width).unwrap_or(u16::MAX)),
            Constraint::Percentage(70),
            Constraint::Min(20),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct ContainerItem(pub crate::container::Container);

impl Pickable for ContainerItem {
    fn title() -> &'static str {
        "Devcontainers"
    }

    fn headers() -> &'static [&'static str] {
        &["Container ID", "Status", "Image", "Project Path"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.0.short_id.clone(),
            self.0.status.clone(),
            self.0.image.clone(),
            self.0.local_folder.clone(),
        ]
    }

    fn search_fields(&self) -> Vec<String> {
        vec![
            self.0.short_id.clone(),
            self.0.status.clone(),
            self.0.image.clone(),
            self.0.local_folder.clone(),
            self.0.config_file.clone(),
        ]
    }

    fn status_lines(&self) -> Vec<String> {
        vec![format!("Config: {}", self.0.config_file)]
    }

    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let status_width = max_widths.get(1).copied().unwrap_or(6).clamp(6, 30);
        vec![
            Constraint::Min(13),
            Constraint::Min(u16::try_from(status_width).unwrap_or(6)),
            Constraint::Percentage(40),
            Constraint::Percentage(60),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct ConfigItem(pub crate::config_store::ConfigEntry);

impl Pickable for ConfigItem {
    fn title() -> &'static str {
        "Configs"
    }

    fn headers() -> &'static [&'static str] {
        &["Name", "Description", "Path"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.0.name.clone(),
            self.0.description.as_deref().unwrap_or("").to_string(),
            self.0.root.display().to_string(),
        ]
    }

    fn search_fields(&self) -> Vec<String> {
        vec![
            self.0.name.clone(),
            self.0.description.clone().unwrap_or_default(),
            self.0.root.to_string_lossy().to_string(),
        ]
    }

    fn status_lines(&self) -> Vec<String> {
        vec![]
    }

    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let name_width = max_widths.first().copied().unwrap_or(4).clamp(4, 30);
        let description_width = max_widths.get(1).copied().unwrap_or(4).clamp(4, 40);
        vec![
            Constraint::Min(u16::try_from(name_width).unwrap_or(4)),
            Constraint::Min(u16::try_from(description_width).unwrap_or(4)),
            Constraint::Percentage(70),
        ]
    }
}

#[derive(Clone, Debug)]
pub struct DevContainerItem(pub crate::workspace::DevContainer);

impl Pickable for DevContainerItem {
    fn title() -> &'static str {
        "Select Dev Container"
    }

    fn headers() -> &'static [&'static str] {
        &["Name", "Config Path"]
    }

    fn cells(&self) -> Vec<String> {
        vec![
            self.0.name.as_deref().unwrap_or("(unnamed)").to_string(),
            self.0.config_path.display().to_string(),
        ]
    }

    fn search_fields(&self) -> Vec<String> {
        vec![
            self.0.name.clone().unwrap_or_default(),
            self.0.config_path.to_string_lossy().to_string(),
        ]
    }

    fn status_lines(&self) -> Vec<String> {
        vec![format!("Workspace: {}", self.0.workspace_path_in_container)]
    }

    fn column_constraints(max_widths: &[usize]) -> Vec<Constraint> {
        let name_width = max_widths.first().copied().unwrap_or(9).clamp(9, 40);
        vec![
            Constraint::Min(u16::try_from(name_width).unwrap_or(9)),
            Constraint::Percentage(80),
        ]
    }
}
