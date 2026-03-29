#!/usr/bin/env bash
set -euo pipefail

mkdir -p /home/dev/.ssh
printf '%s\n' "$AUTHORIZED_KEY" > /home/dev/.ssh/authorized_keys
chown -R dev:dev /home/dev/.ssh
chmod 700 /home/dev/.ssh
chmod 600 /home/dev/.ssh/authorized_keys

exec /usr/sbin/sshd -D -e
