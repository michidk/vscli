FROM ubuntu:24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openssh-server \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/run/sshd \
    && useradd -m -s /bin/bash dev \
    && mkdir -p /home/dev/workspace/.devcontainer \
    && printf '%s\n' \
        '{' \
        '  "name": "remote-test",' \
        '  "image": "mcr.microsoft.com/devcontainers/base:ubuntu"' \
        '}' > /home/dev/workspace/.devcontainer/devcontainer.json \
    && chown -R dev:dev /home/dev

COPY --chmod=755 tests/remote-host/entrypoint.sh /usr/local/bin/vscli-remote-host-entrypoint

ENTRYPOINT ["/usr/local/bin/vscli-remote-host-entrypoint"]
