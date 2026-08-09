use super::{ConfigEntry, ConfigStore};
use crate::opts::ConfigAction;
use crate::ui;
use color_eyre::eyre::Result;
use log::info;
use std::io::Write;

fn print_entries(entries: &[ConfigEntry], long: bool) {
    if long {
        let name_width = entries
            .iter()
            .map(|entry| entry.name.len())
            .max()
            .unwrap_or(4);
        let desc_width = entries
            .iter()
            .map(|entry| entry.description.as_deref().unwrap_or("").len())
            .max()
            .unwrap_or(4);
        println!("{:<name_width$}  {:<desc_width$}  PATH", "NAME", "DESC");
        for entry in entries {
            println!(
                "{:<name_width$}  {:<desc_width$}  {}",
                entry.name,
                entry.description.as_deref().unwrap_or(""),
                entry.root.display()
            );
        }
    } else {
        for entry in entries {
            println!("{}", entry.name);
        }
    }
}

pub fn run_command(action: ConfigAction, store: &ConfigStore, editor: &str) -> Result<()> {
    match action {
        ConfigAction::Ui => {
            let entries = store.list();
            if entries.is_empty() {
                println!("(no configs)");
                return Ok(());
            }
            let mut delete_cb = |item: &ui::ConfigItem| {
                if let Err(error) = store.rm(&item.0.name) {
                    log::warn!("Failed to remove config '{}': {error}", item.0.name);
                }
            };
            let selected =
                ui::pick_config(entries, ui::PickerOpts::default(), Some(&mut delete_cb))?;
            if let Some(config) = selected {
                info!("Opening config '{}' for editing...", config.name);
                std::process::Command::new(editor)
                    .arg(&config.root)
                    .output()?;
            }
        }
        ConfigAction::List { long } => {
            let entries = store.list();
            if entries.is_empty() {
                println!("(no configs)");
                return Ok(());
            }
            print_entries(&entries, long);
        }
        ConfigAction::Dir => println!("{}", store.dir().display()),
        ConfigAction::Add { name } => {
            let root = store.add(&name)?;
            info!("Created config '{}' at {}", name, root.display());
        }
        ConfigAction::Copy { name, path } => {
            let target_dir = path.canonicalize().unwrap_or(path);
            store.copy_into(&name, &target_dir)?;
            info!("Copied config '{}' into {}", name, target_dir.display());
        }
        ConfigAction::Rm { name } => {
            let root = store.dir().join(&name);
            eprint!("Remove config '{name}' at {}? [y/N] ", root.display());
            std::io::stderr().flush()?;
            let mut answer = String::new();
            std::io::stdin().read_line(&mut answer)?;
            if answer.trim().eq_ignore_ascii_case("y") {
                store.rm(&name)?;
                info!("Removed config '{name}'");
            }
        }
    }
    Ok(())
}
