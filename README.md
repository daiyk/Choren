<div align="center">

<img src="app/src-tauri/icons/128x128@2x.png" width="112" alt="Choren logo">

# Choren

**A modern, open-source studio for NAO robot behaviors, compatible with Choregraphe projects.**

Open, edit and save `.pml` projects, `.crg` archives and `.xar` behaviors, run them on a
built-in virtual NAO 6, and connect to real robots.

[![Release](https://img.shields.io/github/v/release/daiyk/Choren?include_prereleases&sort=semver&color=2f80ed&logo=github)](https://github.com/daiyk/Choren/releases)
[![Build](https://img.shields.io/github/actions/workflow/status/daiyk/Choren/release.yml?logo=githubactions&logoColor=white&label=release%20build)](https://github.com/daiyk/Choren/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-22c55e)](LICENSE)
[![Stars](https://img.shields.io/github/stars/daiyk/Choren?style=flat&logo=github&color=f5c518)](https://github.com/daiyk/Choren/stargazers)
[![Last commit](https://img.shields.io/github/last-commit/daiyk/Choren?color=8b5cf6)](https://github.com/daiyk/Choren/commits/main)

![Rust](https://img.shields.io/badge/Rust-2024-dea584?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2-24c8db?logo=tauri&logoColor=white)
![React](https://img.shields.io/badge/React-19-61dafb?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-strict-3178c6?logo=typescript&logoColor=white)
![Python](https://img.shields.io/badge/Python-3.8%2B-3776ab?logo=python&logoColor=white)
![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows-lightgrey?logo=apple)

![Lines of code](https://img.shields.io/badge/lines%20of%20code-38.8k-0ea5e9)
![Tests](https://img.shields.io/badge/tests-270%2B-16a34a)
![NAOqi services](https://img.shields.io/badge/NAOqi%202.8%20services-108-f97316)
![API methods](https://img.shields.io/badge/API%20methods%20described-4%2C569-ef4444)

</div>

<p align="center">
  <img src="assets/screenshots/choren-studio-light.png" alt="Choren studio: project panel, flow diagram and inspector" width="880">
</p>

## At a glance

| | |
|---|---|
| 🦀 **Rust** | ~18.9k lines across four crates and the native shell |
| ⚛️ **TypeScript / React** | ~17.1k lines of studio UI |
| 🐍 **Python** | Box-script host and `naoqi` / `qi` compatibility modules |
| ✅ **Tests** | 125 Rust · 104 Playwright UI · 38 Vitest · Python host tests |
| 🤖 **Virtual robot** | NAO 6 over the libqi wire protocol, with a schema of 108 NAOqi 2.8 services |
| 📦 **Box library** | 11 original Choren boxes in 7 categories, plus your own Choregraphe libraries |

## Features

- **Choregraphe-compatible files.** Reads and writes `.pml`, `.crg` and `.xar`, keeping
  imported coordinates, port semantics and link geometry so saved files stay compatible.
- **Visual authoring.** Dockable workspace with flow diagrams, timelines, an inspector, an
  outline, undo/redo backed by the project history, and a CodeMirror Python script editor.
- **Virtual NAO 6.** Run behaviors without hardware. Box scripts run in a real Python host
  against a simulated NAOqi that speaks libqi (QiMessaging), the protocol real robots use.
- **Real robots.** Discover and connect to NAO over the network (port 9561 by default),
  browse robot files over SFTP, watch memory keys and filter robot logs.
- **Light and dark themes**, scalable text, native menus and signed auto-updates on macOS.

<p align="center">
  <img src="assets/screenshots/choren-studio-dark.png" alt="Choren studio in dark mode" width="720">
</p>

## Repository layout

| Path | What's inside |
|---|---|
| [`app/`](app) | Desktop studio (Tauri 2, React 19). See [app/README.md](app/README.md). |
| [`crates/choren-core`](crates/choren-core) | Project model, validation rules and the edit engine |
| [`crates/choren-qi`](crates/choren-qi) | libqi (QiMessaging) protocol: client, server, values, signatures |
| [`crates/choren-naoqi`](crates/choren-naoqi) | Virtual robot and behavior runtime |
| [`crates/choren-robot`](crates/choren-robot) | Robot descriptors and posture data |
| [`python/choren_host`](python/choren_host) | Python host that runs box scripts |
| [`library/choren`](library/choren) | Choren's own starter box library |

## Getting started

Prerequisites: Node 24, a recent stable Rust, the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS, and Python 3.8+
as `python3` on `PATH` for running behaviors on the virtual robot.

```sh
cd app
npm ci
npm run tauri dev
```

Run the test suites:

```sh
cargo test --workspace                              # from the repo root
cd app && npm test && npm run test:ui               # unit and browser UI tests
python3 -m unittest discover -s python/tests        # from the repo root
```

To use Choregraphe's robot meshes, box library and icons, point **Settings** at your own
Choregraphe installation. See [app/README.md](app/README.md) for the full feature tour and
development notes.

## ⚖️ Legal

Choren is an independent open-source project that is built to stay on the right side of the
law:

- **No Choregraphe code or assets are distributed.** This repository contains no
  Choregraphe source code, binaries, robot meshes, icons or box library. Those stay in the
  user's own Choregraphe installation and are loaded from there only if the user points
  Choren at it.
- **Original implementation for interoperability.** Choren's code is its own. It implements
  the Choregraphe file formats and the NAOqi messaging protocol so that people can keep
  working with the projects and robots they already have.
- **Open-source foundations.** [`crates/choren-qi`](crates/choren-qi) is based on
  [libqi](https://github.com/aldebaran/libqi), © 2011-2014 Aldebaran Robotics, released
  under the BSD 3-Clause License; its notice is kept in
  [`crates/choren-qi/LICENSE-libqi`](crates/choren-qi/LICENSE-libqi). All other
  dependencies are used under their own open-source licenses.
- **Trademarks.** Choren is not affiliated with, endorsed by or sponsored by Aldebaran,
  SoftBank Robotics or SoftBank Group. Choregraphe, NAO, NAOqi and Pepper are trademarks of
  their respective owners and are used here only to describe compatibility.

If you hold rights in any of the above and have a concern, please
[open an issue](https://github.com/daiyk/Choren/issues) and it will be addressed promptly.

## License

Choren is released under the [MIT License](LICENSE).

## Star history

<a href="https://star-history.com/#daiyk/Choren&Date">
  <img src="https://api.star-history.com/svg?repos=daiyk/Choren&type=Date" alt="Star history chart" width="600">
</a>
