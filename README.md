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
alias vs="vscli --insiders"
```

## Usage

### Commandline

After installation, the `vscli` command will be available:

```sh
USAGE:
    vscli [FLAGS] [OPTIONS] <path> [args]...

FLAGS:
    -h, --help        Prints help information
    -i, --insiders    Whether to launch the insiders version of vscode
    -V, --version     Prints version information

OPTIONS:
    -b, --behaviour <behaviour>    Launch behaviour [default: detect]  [possible values: detect, force-container, force-
                                   classic]
    -v, --verbosity <verbosity>    The verbosity of the output [default: info]

ARGS:
    <path>       The path of the vscode project to open
    <args>...    Input arguments to pass to vscode
```
