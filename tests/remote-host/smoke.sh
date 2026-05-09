#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
TEST_DIR="$ROOT_DIR/tests/remote-host"
TMP_DIR="$(mktemp -d)"
KEY_PATH="$TMP_DIR/id_ed25519"
SSH_CONFIG="$TMP_DIR/ssh_config"
DOCKER_CONFIG_DIR="$TMP_DIR/docker-config"
FAKE_EDITOR="$TMP_DIR/fake-editor.sh"
EDITOR_LOG="$TMP_DIR/editor-args.log"

cleanup() {
  AUTHORIZED_KEY="${AUTHORIZED_KEY:-}" docker compose -f "$TEST_DIR/docker-compose.yml" down -v >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$DOCKER_CONFIG_DIR"
export DOCKER_CONFIG="$DOCKER_CONFIG_DIR"

ssh-keygen -q -t ed25519 -N "" -f "$KEY_PATH" >/dev/null
AUTHORIZED_KEY="$(cat "$KEY_PATH.pub")"
export AUTHORIZED_KEY

cat > "$SSH_CONFIG" <<EOF
Host vscli-remote-test
    HostName 127.0.0.1
    Port 2222
    User dev
    IdentityFile $KEY_PATH
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
EOF

cat > "$FAKE_EDITOR" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$VSCLI_EDITOR_LOG"
EOF
chmod +x "$FAKE_EDITOR"

docker compose -f "$TEST_DIR/docker-compose.yml" up -d --build

for _ in $(seq 1 30); do
  if ssh -F "$SSH_CONFIG" vscli-remote-test 'test -f /home/dev/workspace/.devcontainer/devcontainer.json'; then
    break
  fi
  sleep 1
done

ssh -F "$SSH_CONFIG" vscli-remote-test 'test -f /home/dev/workspace/.devcontainer/devcontainer.json'
VSCLI_EDITOR_LOG="$EDITOR_LOG" cargo run -- open --remote-host vscli-remote-test /home/dev/workspace --command "$FAKE_EDITOR"

grep -Fx -- '--folder-uri' "$EDITOR_LOG"
grep -Fx -- 'vscode-remote://ssh-remote+vscli-remote-test/home/dev/workspace' "$EDITOR_LOG"
