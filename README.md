# vbotssh

`vbotssh` is a Rust TUI monitor for Linux hosts. It shows local and remote hosts in one interface, polls remote machines over SSH, and renders CPU, temperature, RAM, disks, network throughput, and Docker container status.

## Features

- local host metrics with immediate first paint;
- remote host polling from `~/.ssh/config`;
- separate refresh intervals for local and remote hosts;
- Catppuccin Mocha themed terminal UI;
- Docker status widget with unhealthy containers highlighted;
- packaging for NixOS, Debian-based systems, and Arch Linux.

## Runtime requirements

`vbotssh` reads several metrics by calling standard Linux tools:

- `ssh` from OpenSSH for remote polling;
- `ping` from `iputils` when ping pre-check is enabled;
- `lsblk` from `util-linux` for physical disk layout;
- `docker` optionally, for the Docker widget.

## Configuration

The user config lives at:

```text
~/.config/vbotssh/config.toml
```

Example setup:

```bash
mkdir -p ~/.config/vbotssh
cp assets/config.example.toml ~/.config/vbotssh/config.toml
```

## Running from source

```bash
cargo run --release
```

## Installation

### NixOS / Nix

Build and run directly from the flake:

```bash
nix run github:vrubelroman/vbotssh?ref=v0.1.1
```

Install into the current profile:

```bash
nix profile install github:vrubelroman/vbotssh?ref=v0.1.1
```

`nix profile install` installs `vbotssh` into the current user's Nix profile. It does not edit `configuration.nix`, does not rebuild NixOS, and does not make the package a declarative system package.

### Debian / Ubuntu

Release builds include a `.deb` artifact. Install it with:

```bash
sudo apt install ./vbotssh_<version>_amd64.deb
```

### Arch Linux

A `PKGBUILD` is included in the repository. Build it with:

```bash
makepkg -si
```

## Releases

Tagging a version like `v0.1.0` triggers the GitHub Actions release workflow:

- runs `cargo test`;
- builds the release binary;
- builds the `.deb` package with `cargo-deb`;
- publishes release artifacts to GitHub Releases.
