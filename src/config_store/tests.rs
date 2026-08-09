use super::ConfigStore;
use std::path::PathBuf;
use tempfile::TempDir;

fn copy_fixture() -> (TempDir, ConfigStore, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let store = ConfigStore::new(Some(temp.path().join("store")));
    let target_dir = temp.path().join("target");
    std::fs::create_dir_all(&target_dir).unwrap();
    (temp, store, target_dir)
}

fn assert_overwrite_refused(error: &color_eyre::Report) {
    assert!(
        error
            .to_string()
            .contains("Refusing to overwrite existing path")
    );
}

#[test]
fn copy_into_copies_config_contents_to_target_directory() {
    let (_temp, store, target_dir) = copy_fixture();

    let created_root = store.add("python-dev").unwrap();
    let script_path = created_root.join("scripts").join("setup.sh");
    std::fs::create_dir_all(script_path.parent().unwrap()).unwrap();
    std::fs::write(&script_path, "#!/bin/sh\n").unwrap();

    let copied_path = store.copy_into("python-dev", &target_dir).unwrap();

    assert_eq!(copied_path, target_dir.join(".devcontainer"));
    assert!(
        target_dir
            .join(".devcontainer")
            .join("devcontainer.json")
            .is_file()
    );
    assert!(target_dir.join("scripts").join("setup.sh").is_file());
}

#[test]
fn copy_into_refuses_to_overwrite_existing_paths() {
    let (_temp, store, target_dir) = copy_fixture();

    std::fs::create_dir_all(target_dir.join(".devcontainer")).unwrap();
    std::fs::write(
        target_dir.join(".devcontainer").join("devcontainer.json"),
        "{}\n",
    )
    .unwrap();
    store.add("python-dev").unwrap();

    let error = store.copy_into("python-dev", &target_dir).unwrap_err();

    assert_overwrite_refused(&error);
}

#[test]
fn copy_into_does_not_partially_write_when_nested_conflict_exists() {
    let (_temp, store, target_dir) = copy_fixture();

    let created_root = store.add("python-dev").unwrap();
    let scripts_dir = created_root.join("scripts");
    std::fs::create_dir_all(&scripts_dir).unwrap();
    std::fs::write(created_root.join("README.md"), "template\n").unwrap();
    std::fs::write(scripts_dir.join("setup.sh"), "#!/bin/sh\n").unwrap();

    std::fs::create_dir_all(target_dir.join("scripts")).unwrap();
    std::fs::write(target_dir.join("scripts").join("setup.sh"), "existing\n").unwrap();

    let error = store.copy_into("python-dev", &target_dir).unwrap_err();

    assert_overwrite_refused(&error);
    assert!(!target_dir.join(".devcontainer").exists());
    assert!(!target_dir.join("README.md").exists());
}
