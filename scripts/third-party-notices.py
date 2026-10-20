#!/usr/bin/env python3
"""Writes the license notices the packaged app ships into app/src-tauri/licenses/ (bundled as a
resource, see tauri.conf.json): Choren's own LICENSE, libqi's, and THIRD_PARTY_NOTICES.txt with
the license texts of every Rust crate linked into the app and every npm production package.

Usage: python3 scripts/third-party-notices.py [target-triple]
(default: $TAURI_ENV_TARGET_TRIPLE, else all platforms). Needs cargo and app/node_modules.
"""
import hashlib, json, os, re, shutil, subprocess, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "app/src-tauri/licenses"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|unlicense)([-_.].*)?$", re.I)


def license_texts(folder):
    files = sorted(f for f in Path(folder).iterdir() if f.is_file() and LICENSE_FILE.match(f.name))
    return "\n\n".join(f.read_text(errors="replace").strip() for f in files)


def rust_packages(triple):
    cmd = ["cargo", "metadata", "--format-version", "1", "--locked", "--manifest-path", str(ROOT / "Cargo.toml")]
    if triple:
        cmd += ["--filter-platform", triple]
    meta = json.loads(subprocess.run(cmd, check=True, capture_output=True, text=True).stdout)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    # Everything the app links: normal dependencies from the app crate down (not build/dev ones).
    start = next(p["id"] for p in meta["packages"] if p["name"] == "choren" and p["source"] is None)
    seen, todo = set(), [start]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    for pid in seen:
        p = packages[pid]
        if p["source"] is None:  # Choren's own crates
            continue
        yield p["name"], p["version"], p.get("license") or "see text", p.get("repository") or "", license_texts(Path(p["manifest_path"]).parent)


def npm_packages():
    app = ROOT / "app"
    lock = json.loads((app / "package-lock.json").read_text())
    for path, p in lock["packages"].items():
        folder = app / path
        if not path or p.get("dev") or not folder.is_dir():
            continue
        info = json.loads((folder / "package.json").read_text())
        repo = info.get("repository") or ""
        if isinstance(repo, dict):
            repo = repo.get("url", "")
        yield info["name"], info["version"], p.get("license") or info.get("license") or "see text", repo, license_texts(folder)


def spdx(names):
    """The first SPDX id named in the first of `names` (`  name version (license)  repo`)."""
    return re.split(r"[ ()/]+", names[0].split("(", 1)[1])[0]


def section(title, packages):
    groups = {}
    for name, version, license, repo, text in sorted(set(packages)):
        # Same text with other wrapping is one group; packages without a text stay on their own.
        key = hashlib.sha256(" ".join(text.split()).encode()).hexdigest() if text else name
        groups.setdefault(key, (text, []))[1].append(f"  {name} {version} ({license}){'  ' + repo if repo else ''}")
    out = [f"{'=' * 78}\n{title}\n{'=' * 78}"]
    for text, names in sorted(groups.values(), key=lambda g: g[1][0]):
        out.append("\n".join(names) + "\n\n" + (text or f"  The package ships no license file. License text: https://spdx.org/licenses/{spdx(names)}.html"))
    return "\n\n" + f"\n\n{'-' * 78}\n\n".join(out)


def main():
    triple = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("TAURI_ENV_TARGET_TRIPLE")
    OUT.mkdir(parents=True, exist_ok=True)
    shutil.copy(ROOT / "LICENSE", OUT / "LICENSE-Choren.txt")
    shutil.copy(ROOT / "crates/choren-qi/LICENSE-libqi", OUT / "LICENSE-libqi.txt")
    # The CPython that scripts/fetch-python-runtime.sh bundled, when this is a release build.
    cpython = next((OUT.parent / "python-runtime").rglob("LICENSE.txt"), None)
    if cpython:
        shutil.copy(cpython, OUT / "LICENSE-CPython.txt")
    notices = (
        "Choren third-party notices\n\n"
        "Choren is distributed under the BSD 3-Clause License (LICENSE-Choren.txt). Its libqi protocol\n"
        "code is based on libqi, (c) 2011-2014 Aldebaran Robotics, BSD 3-Clause (LICENSE-libqi.txt).\n"
        "The bundled Python runtime is CPython from python-build-standalone, under the Python\n"
        "Software Foundation License (LICENSE-CPython.txt).\n"
        "Choren also includes the third-party software listed below, each under the license text that\n"
        "follows it. The source of each package is available from the repository shown, at the\n"
        "version shown.\n"
    )
    notices += section("Rust crates", rust_packages(triple))
    notices += section("JavaScript packages (npm)", npm_packages())
    (OUT / "THIRD_PARTY_NOTICES.txt").write_text(notices + "\n")
    print(f"third-party-notices: wrote {OUT / 'THIRD_PARTY_NOTICES.txt'} ({len(notices) // 1024} KiB)")


if __name__ == "__main__":
    main()
