# Minecraft Source Code Tracker

Automatically decompiles each version of Minecraft Java Edition and commits the source code to a git repository, creating tags for each version.

## Overview

This tool monitors Mojang's Piston Meta API for new Minecraft releases (both snapshots and full releases), downloads the client JAR, decompiles it using [Vineflower](https://github.com/Vineflower/vineflower), and pushes the decompiled source to a git repository. Each version is tagged, making it easy to browse and compare changes between Minecraft versions.

## Features

- Automatic detection of new Minecraft versions via Piston Meta API
- Decompiles client JAR files using Vineflower decompiler
- Commits decompiled source to a configurable git repository
- Creates git tags for each processed version
- Runs continuously with hourly checks for new versions
- Tracks processed versions via git tags to avoid reprocessing

## Requirements

- Rust 1.91.0 or later
- Java Runtime Environment (JRE) - required for Vineflower decompiler
- Git repository with push access

## Installation

```bash
git clone https://github.com/your-username/minecraft-sourcecode-tracker.git
cd minecraft-sourcecode-tracker
cargo build --release
```

## Usage

```bash
minecraft-sourcecode-tracker \
  --git-username <USERNAME> \
  --git-auth-token <TOKEN> \
  --git-url <REPOSITORY_URL> \
  [--git-email <EMAIL>]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `--git-username` | Yes | Username for git authentication |
| `--git-auth-token` | Yes | Personal access token or password for git authentication |
| `--git-url` | Yes | URL of the git repository (e.g., `https://github.com/user/repo.git`) |
| `--git-email` | No | Email for git commits (defaults to `<username>@users.noreply.github.com`) |

### Example

```bash
minecraft-sourcecode-tracker \
  --git-username drew-chase \
  --git-auth-token ghp_xxxxxxxxxxxx \
  --git-url https://github.com/drew-chase/minecraft-source-code.git
```

## How It Works

1. **Fetch Version List**: Queries Mojang's Piston Meta API for all available Minecraft versions
2. **Check Processed Versions**: Compares available versions against git tags in the repository
3. **Process New Versions**: For each unprocessed version:
   - Downloads the client JAR from Mojang's servers
   - Decompiles the JAR using Vineflower
   - Commits the decompiled source to the `client` branch
   - Creates a git tag with the version name (e.g., `1.21.4`, `25w03a`)
   - Pushes the commit and tag to the remote repository
4. **Wait**: Sleeps for 1 hour before checking for new versions again

## Repository Structure

The target repository will have:
- A `client` branch containing the decompiled Minecraft client source
- Tags for each processed version

## License

This project is for educational purposes. Minecraft is a trademark of Mojang Studios. The decompiled source code is subject to Minecraft's EULA and should only be used for learning and modding purposes.

## Author

Drew Chase
