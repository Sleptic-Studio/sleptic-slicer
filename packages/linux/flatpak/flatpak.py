#!/usr/bin/env python3
# Copyright (c) 2026 Sleptic
# SPDX-License-Identifier: LicenseRef-Sleptic-Source-1.0
"""Sleptic Slicer's Flatpak packaging, in one tool.

    python3 flatpak.py check         are the vendored source lists in step with Cargo.lock?
    python3 flatpak.py vendor-cargo  rewrite cargo-sources.json from Cargo.lock
    python3 flatpak.py vendor-skia   rewrite skia-binaries.source.json for the pinned Skia
    python3 flatpak.py build         check, then build both packages into .cache/*.flatpak

A Flatpak build has no network, so everything it compiles is declared up
front: every registry crate in Cargo.lock (cargo-sources.json, written by
Flatpak's own flatpak-cargo-generator) and the prebuilt Skia library
skia-bindings links (skia-binaries.source.json). `check` compares both with
the lock file and is run before every build.

`build` copies only the files a compile reads into .cache/src, packs them as a
reproducible tar, and hands flatpak-builder an `archive` source with that
tar's sha256. flatpak-builder keys its cache on declared checksums, so an
unchanged tree builds nothing and any change rebuilds. Bundles land in
.cache/ and nothing is installed (`flatpak install --user <bundle>`).
"""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CACHE = HERE / ".cache"
LOCK = ROOT / "Cargo.lock"
CARGO_LIST = HERE / "cargo-sources.json"
SKIA_LIST = HERE / "skia-binaries.source.json"
APPS = ["com.sleptic.slicer", "com.sleptic.slicer.cli"]
METAINFO = "com.sleptic.slicer.metainfo.xml"

# flatpak-builder-tools' cargo generator, pinned by commit and checksum.
GENERATOR = {
    "commit": "737c0085912f9f7dabf9341d4608e2a77a51a73a",
    "sha256": "b373c8ab1a05378ec5d8ed0645c7b127bcec7d2f7a1798694fbc627d570d856c",
}

# What the compile needs from the tree; everything else is left out so an
# unrelated edit (docs, scripts, other platforms' packaging) keeps the cache.
# Tests are left out too, which holds while src-side test modules that read
# ../tests stay behind #[cfg(test)].
SKIP_TOP = {"target", "scripts", "tarpaulin.toml", "local"}
SKIP_ANY = {"tests", ".cache", ".flatpak-builder"}
SKIP_PACKAGES = {"linux", "macos", "windows"}


def fail(message):
    sys.exit(f"flatpak: {message}")


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for block in iter(lambda: fh.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def locked_packages():
    """(name, version, from_registry) for each [[package]] in Cargo.lock."""
    packages = []
    for block in LOCK.read_text(encoding="utf-8").split("[[package]]")[1:]:
        fields = dict(re.findall(r'^(name|version|source) = "([^"]*)"', block, re.M))
        packages.append((fields.get("name"), fields.get("version"), fields.get("source", "").startswith("registry+")))
    return packages


def check():
    """True when both vendored lists match Cargo.lock; prints what differs."""
    for path in (LOCK, CARGO_LIST, SKIA_LIST):
        if not path.is_file():
            fail(f"missing {path}")
    current = True

    wanted = {f"cargo/vendor/{n}-{v}" for n, v, registry in locked_packages() if registry}
    declared = {e["dest"] for e in json.loads(CARGO_LIST.read_text()) if e.get("type") == "archive"}
    if wanted != declared:
        current = False
        print("cargo-sources.json differs from Cargo.lock:", file=sys.stderr)
        for d in sorted(wanted - declared):
            print(f"  missing from the list: {d.split('/')[-1]}", file=sys.stderr)
        for d in sorted(declared - wanted):
            print(f"  no longer locked:      {d.split('/')[-1]}", file=sys.stderr)
        print("  refresh it with: python3 flatpak.py vendor-cargo", file=sys.stderr)

    # The archive's release tag is the skia-bindings version. Its file name
    # also encodes features and a configuration hash, which only a build of
    # skia-bindings can work out; a feature change is not caught here.
    skia = next((v for n, v, _ in locked_packages() if n == "skia-bindings"), None)
    url = json.loads(SKIA_LIST.read_text())[0].get("url", "")
    tag = re.search(r"/releases/download/([^/]+)/", url)
    if not skia or not tag:
        current = False
        print("could not read the skia-bindings version from Cargo.lock or the Skia list", file=sys.stderr)
    elif tag.group(1) != skia:
        current = False
        print(f"skia-binaries.source.json is for {tag.group(1)}, Cargo.lock has skia-bindings {skia}", file=sys.stderr)
        print("  refresh it with: python3 flatpak.py vendor-skia", file=sys.stderr)

    if current:
        print("vendored sources match Cargo.lock")
    return current


def download(url, target, expect=None):
    tmp = target.with_suffix(target.suffix + ".part")
    target.parent.mkdir(parents=True, exist_ok=True)
    if not url.startswith("https://"):
        fail(f"refusing a non-https download: {url}")
    with urllib.request.urlopen(url) as response, open(tmp, "wb") as out:
        shutil.copyfileobj(response, out)
    if expect and sha256(tmp) != expect:
        tmp.unlink()
        fail(f"{url} does not have the pinned checksum {expect}")
    tmp.replace(target)


def vendor_cargo():
    if not shutil.which("uv"):
        fail("needs uv, which runs the generator with its own dependencies (https://docs.astral.sh/uv/)")
    script = CACHE / f"flatpak-cargo-generator-{GENERATOR['commit']}.py"
    if not script.is_file() or sha256(script) != GENERATOR["sha256"]:
        url = ("https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/"
               f"{GENERATOR['commit']}/cargo/flatpak-cargo-generator.py")
        download(url, script, GENERATOR["sha256"])
    subprocess.run(["uv", "run", "--no-project", str(script), str(LOCK), "-o", str(CARGO_LIST)], check=True)
    print(f"wrote {CARGO_LIST.name}")


def vendor_skia():
    # skia-bindings' build script prints the archive it fetches ("FROM: <url>").
    # Ask cargo for this workspace's own build-script runs, rather than
    # searching target/, which keeps every configuration ever built there.
    run = subprocess.run(["cargo", "build", "--manifest-path", str(ROOT / "Cargo.toml"), "--message-format=json"],
                         capture_output=True, text=True)
    if run.returncode:
        fail("the workspace does not build, so the Skia archive it needs is unknown")
    urls = set()
    for line in run.stdout.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") == "build-script-executed" and re.search(r"(^|[#/])skia-bindings[@ ]", message.get("package_id", "")):
            output = Path(message["out_dir"]).parent / "output"
            if output.is_file():
                urls.update(m.strip() for m in re.findall(r"^\s*FROM:\s*(\S+)", output.read_text(errors="replace"), re.M))
    if len(urls) != 1:
        fail(f"expected one Skia archive URL from skia-bindings, found {sorted(urls) or 'none'}")
    url = urls.pop()
    archive = CACHE / url.rsplit("/", 1)[-1]
    try:
        download(url, archive)
    except OSError as err:
        fail(f"no prebuilt Skia for this configuration ({err}); building Skia from source is not supported")
    entry = {"type": "file", "url": url, "sha256": sha256(archive), "dest-filename": "skia-binaries.tar.gz"}
    SKIA_LIST.write_text(json.dumps([entry], indent=2) + "\n")
    print(f"wrote {SKIA_LIST.name}; the build links it with SKIA_BINARIES_URL=file://$PWD/skia-binaries.tar.gz")


def wanted(rel):
    parts = rel.parts
    if parts[0] in SKIP_TOP or parts[0].startswith("."):
        return False
    if len(parts) > 1 and parts[0] == "packages" and parts[1] in SKIP_PACKAGES:
        return False
    return not (SKIP_ANY & set(parts)) and rel.suffix != ".md"


def source_list(name, entry):
    path = CACHE / name
    path.write_text(json.dumps([entry], indent=2) + "\n")


def build():
    if not check():
        fail("the vendored sources are stale; refresh them first")
    CACHE.mkdir(exist_ok=True)

    metainfo = CACHE / METAINFO
    subprocess.run([sys.executable, str(ROOT / "scripts/render_metainfo.py"),
                    str(ROOT / "packages/shared" / METAINFO), str(ROOT / "apps/slicer-gui/Cargo.toml"), str(metainfo)],
                   check=True)
    source_list("metainfo.source.json", {"type": "file", "path": f".cache/{METAINFO}", "sha256": sha256(metainfo)})

    # A fixed order, owner and time, so the same tree gives the same bytes
    # (and the same checksum) every run.
    tar_path = CACHE / "source.tar"
    files = sorted(p for p in ROOT.rglob("*") if p.is_file() and wanted(p.relative_to(ROOT)))
    with tarfile.open(tar_path, "w", format=tarfile.GNU_FORMAT) as tar:
        for path in files:
            info = tar.gettarinfo(str(path), arcname=str(path.relative_to(ROOT)))
            info.mtime, info.uid, info.gid, info.uname, info.gname = 1577836800, 0, 0, "", ""
            with open(path, "rb") as fh:
                tar.addfile(info, fh)
    # Paths in an included list resolve against the manifest's folder.
    source_list("source.source.json", {"type": "archive", "path": ".cache/source.tar",
                                       "sha256": sha256(tar_path), "strip-components": 0})

    repo = CACHE / "repo"
    for app in APPS:
        print(f"building {app}")
        subprocess.run(["flatpak-builder", "--user", "--force-clean", f"--state-dir={CACHE / 'flatpak-builder'}",
                        f"--repo={repo}", str(CACHE / "build" / app), str(HERE / f"{app}.yml")], check=True)
    for app in APPS:
        bundle = CACHE / f"{app}.flatpak"
        subprocess.run(["flatpak", "build-bundle", str(repo), str(bundle), app], check=True)
        print(f"bundle: {bundle}")
    print("install a bundle with: flatpak install --user <bundle>")


def main():
    commands = {"check": lambda: sys.exit(0 if check() else 1), "vendor-cargo": vendor_cargo,
                "vendor-skia": vendor_skia, "build": build}
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=commands)
    commands[parser.parse_args().command]()


if __name__ == "__main__":
    main()
