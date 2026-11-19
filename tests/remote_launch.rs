#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn cli(temp: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vscli"));
    command
        .env_clear()
        .arg("--history-path")
        .arg(temp.join("history.json"))
        .arg("--config-dir")
        .arg(temp.join("configs"));
    command
}

#[test]
fn remote_launch_passes_exact_editor_arguments_and_records_history() {
    let temp = tempfile::tempdir().unwrap();
    let editor = temp.path().join("editor");
    let log = temp.path().join("arguments");
    std::fs::write(
        &editor,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$EDITOR_LOG\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();

    let output = cli(temp.path())
        .env("EDITOR_LOG", &log)
        .args([
            "open",
            "--remote-host",
            "build-server",
            "/remote-only/my project",
            "--command",
        ])
        .arg(&editor)
        .args(["--", "--reuse-window"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        std::fs::read_to_string(log).unwrap(),
        "--reuse-window\n--folder-uri\nvscode-remote://ssh-remote+build-server/remote-only/my project\n"
    );
    let history: serde_json::Value =
        serde_json::from_slice(&std::fs::read(temp.path().join("history.json")).unwrap()).unwrap();
    assert_eq!(history[0]["remote_host"], "build-server");
    assert_eq!(history[0]["workspace_path"], "/remote-only/my project");
    assert_eq!(history[0]["behavior"]["strategy"], "ForceClassic");
    assert!(history[0]["config_path"].is_null());
}

#[test]
fn remote_launch_rejects_unsupported_options_before_starting_an_editor() {
    let temp = tempfile::tempdir().unwrap();
    for (args, message) in [
        (
            vec!["/remote-only/project", "--behavior", "force-container"],
            "--behavior force-container is not supported",
        ),
        (
            vec!["/remote-only/project", "--behavior", "detect"],
            "--behavior detect is not supported",
        ),
        (vec!["relative/project"], "Remote path must be absolute"),
        (
            vec!["/remote-only/project", "--config", "missing-config"],
            "--config cannot be combined with --remote-host",
        ),
        (
            vec![
                "/remote-only/project",
                "--",
                "--folder-uri",
                "file:///other",
            ],
            "Specifying `--folder-uri` is not possible",
        ),
    ] {
        let output = cli(temp.path())
            .args(["open", "--remote-host", "build-server"])
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "{:?}",
            output
        );
        assert!(!temp.path().join("history.json").exists());
    }
}
