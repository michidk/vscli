# vscli

[![MIT License](https://img.shields.io/crates/l/vscli)](https://choosealicense.com/licenses/mit/) [![Continuous integration](https://github.com/michidk/vscli/workflows/Continuous%20Integration/badge.svg)](https://github.com/michidk/vscli/actions) [![Crates.io](https://img.shields.io/crates/v/vscli)](https://crates.io/crates/vscli) [![Homebrew](https://img.shields.io/badge/homebrew-available-blue?style=flat)](https://github.com/michidk/homebrew-tools/blob/main/Formula/vscli.rb)

A CLI tool to launch vscode projects, which supports [devcontainers](https://containers.dev/).

## Features

- A shorthand for launching vscode projects
- Detects whether a project is a [devcontainers](https://containers.dev/) project, and launches the devcontainer instead
- Supports the [insiders](https://code.visualstudio.com/insiders/) version of vscode

## Installation

### Cargo

Install `vscli` using [cargo](https://doc.rust-lang.org/cargo/) on Windows or Linux:

```sh
cargo install vscli
```

### Homebrew

Install `vscli` using [brew](https://brew.sh/) on Linux:

```sh
brew install michidk/tools/vscli
```

### Additional steps

You can set a shorthand alias for `vscli` in your shell's configuration file:

```sh
alias vs="vscli --verbosity error"
```

## Usage

### Commandline

After installation, the `vscli` command will be available:

```sh
Usage: vscli [OPTIONS] [PATH] [ARGS]...

Arguments:
  [PATH]
          The path of the vscode project to open

          [default: .]

  [ARGS]...
          Aditional arguments to pass to vscode

Options:
  -b, --behaviour <BEHAVIOUR>
          Launch behaviour

          [default: detect]

          Possible values:
          - detect:          Use devcontainer if it was detected
          - force-container: Force open with devcontainer, even if no config was found
          - force-classic:   Ignore devcontainers

  -i, --insiders
          Whether to launch the insiders version of vscode

  -v, --verbosity <VERBOSITY>
          The verbosity of the output

          [default: info]

  -h, --help
          Print help (see a summary with '-h')
```

### Examples

You can launch a project using the default behaviour:

```sh
vscli                               # open vscode in the current directory
vscli .                             # open vscode in the current directory
vscli /path/to/project              # open vscode in the specified directory
```

The default behaviour tries to dectect whether the project is a [devcontainers](https://containers.dev/) project. If it is, it will launch the devcontainer instead - if not it will launch vscode normally.

You can change the launch behvaiour using the `--behaviour` flag:

```sh
vscli --behaviour force-container . # force open vscode devcontainer (even if vscli did not detect a devcontainer)
vscli --behaviour force-classic .   # force open vscode without a devcontairer (even if vscli did detect a devcontainer)
```

You can launch the insiders version of vscode using the `--insiders` flag:

```sh
vscli --insiders .                  # open vscode insiders in the current directory
```

Additional arguments can be passed to the `code` executable, by specifying them after `--`:

```sh
vscli . -- --disable-gpu            # open vscode in the current directory without GPU hardware acceleration
```

Read more about the `code` flags, by executing `code --help`.
