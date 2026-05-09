FROM ubuntu:24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y openssh-server sudo \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/run/sshd

RUN useradd -m -s /bin/bash dev \
    && mkdir -p /home/dev/workspace/.devcontainer \
    && printf '{\n  "name": "remote-test",\n  "image": "mcr.microsoft.com/devcontainers/base:ubuntu"\n}\n' > /home/dev/workspace/.devcontainer/devcontainer.json \
    && chown -R dev:dev /home/dev

COPY tests/remote-host/entrypoint.sh /usr/local/bin/vscli-remote-host-entrypoint
RUN chmod +x /usr/local/bin/vscli-remote-host-entrypoint

EXPOSE 22

ENTRYPOINT ["/usr/local/bin/vscli-remote-host-entrypoint"]
