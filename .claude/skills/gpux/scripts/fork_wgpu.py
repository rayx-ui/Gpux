"""Recreate the gpux-wgpu fork in vendor/wgpu from a crates.io wgpu release.

Usage, from the Gpux repository root:

    python .claude/skills/gpux/scripts/fork_wgpu.py <wgpu version> <fork number>

For example `fork_wgpu.py 30.0.1 1` produces version `30.0.1-gpux.1`. The script vendors the
release into a temporary directory, replaces `vendor/wgpu/` with the seven forked crates (library
targets only, forked dependencies renamed and pointed at their siblings) and reapplies the DX12
window swap chain scaling change. It then updates the workspace `wgpu` dependency version; port
gpux to any API changes and validate afterwards.
"""

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

FORKED = [
    "wgpu",
    "wgpu-core",
    "wgpu-hal",
    "wgpu-core-deps-apple",
    "wgpu-core-deps-emscripten",
    "wgpu-core-deps-wasm",
    "wgpu-core-deps-windows-linux-android",
]
DROPPED_FILES = {".cargo-checksum.json", ".cargo_vcs_info.json", "Cargo.toml.orig", "Cargo.lock"}
DROPPED_DIRS = {"examples", "tests", "benches"}
DX12_STRETCH = """                    Scaling: Dxgi::DXGI_SCALING_STRETCH,
                    SwapEffect: Dxgi::DXGI_SWAP_EFFECT_FLIP_DISCARD,"""
DX12_FORK = """                    // gpux: window swap chains present unscaled, so resizing never stretches
                    // the previous frame (https://github.com/gfx-rs/wgpu/issues/10196).
                    Scaling: if matches!(&self.target, SurfaceTarget::WndHandle(_)) {
                        Dxgi::DXGI_SCALING_NONE
                    } else {
                        Dxgi::DXGI_SCALING_STRETCH
                    },
                    SwapEffect: Dxgi::DXGI_SWAP_EFFECT_FLIP_DISCARD,"""


def read(path: Path) -> str:
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n")


def write(path: Path, text: str) -> None:
    path.write_bytes(text.encode("utf-8"))


def vendor(version: str, scratch: Path) -> Path:
    (scratch / "src").mkdir(parents=True)
    write(scratch / "src/lib.rs", "")
    write(scratch / "Cargo.toml", f"""[package]
name = "wgpu-fork-source"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
wgpu = {{ version = "={version}", features = ["webgl"] }}

[workspace]
""")
    subprocess.run(["cargo", "vendor", "--versioned-dirs", "vendor"], cwd=scratch, check=True,
                   stdout=subprocess.DEVNULL)
    return scratch / "vendor"


def fork_manifest(name: str, text: str, version: str, fork: str) -> str:
    lines = text.split("\n")
    header_end = next(i for i, line in enumerate(lines) if line.startswith("["))
    sections, current = [], []
    for line in lines[header_end:]:
        if line.startswith("[") and current:
            sections.append(current)
            current = []
        current.append(line)
    sections.append(current)
    kept = []
    for section in sections:
        head = section[0]
        if re.match(r"\[\[(example|test|bench)\]\]", head) or "dev-dependencies" in head:
            continue
        dep = re.match(r"\[(?:target\.'[^']+'\.)?dependencies\.([\w-]+)\]", head)
        if dep and dep.group(1) in FORKED:
            body, count = re.subn(r'^version = "[^"]+"$',
                                  f'package = "gpux-{dep.group(1)}"\nversion = "={fork}"\npath = "../{dep.group(1)}"',
                                  "\n".join(section[1:]), flags=re.M)
            if count != 1:
                sys.exit(f"{name}: expected one version line in {head}")
            section = [head] + body.split("\n")
        kept.append(section)
    body = "\n".join("\n".join(section) for section in kept)
    replacements = [
        (rf'^name = "{re.escape(name)}"$', f'name = "gpux-{name}"'),
        (rf'^version = "{re.escape(version)}"$', f'version = "{fork}"'),
        (r'^description = "(.*)"$',
         lambda m: f'description = "Temporary gpux fork of {name} {version} (DX12 window swap chains use DXGI_SCALING_NONE, gfx-rs/wgpu#10196). {m.group(1)}"'),
        (r'^repository = "https://github.com/gfx-rs/wgpu"$', 'repository = "https://github.com/rayx-ui/Gpux"'),
    ]
    for pattern, replacement in replacements:
        body, count = re.subn(pattern, replacement, body, count=1, flags=re.M)
        if count != 1:
            sys.exit(f"{name}: no match for {pattern}")
    header = (
        f"# gpux fork of {name} {version} from https://github.com/gfx-rs/wgpu, published as gpux-{name}\n"
        "# until upstream fixes https://github.com/gfx-rs/wgpu/issues/10196. The only behavior change is\n"
        "# in gpux-wgpu-hal: DX12 swap chains for window handles use DXGI_SCALING_NONE. This manifest is\n"
        "# the published (normalized) manifest with forked dependencies renamed and only the library kept.\n\n"
    )
    return header + body.rstrip("\n") + "\n"


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    version, number = sys.argv[1], sys.argv[2]
    fork = f"{version}-gpux.{number}"
    root = Path.cwd()
    destination = root / "vendor/wgpu"
    if not (root / "Cargo.toml").is_file() or not destination.is_dir():
        sys.exit("run from the Gpux repository root")
    with tempfile.TemporaryDirectory() as scratch:
        sources = vendor(version, Path(scratch))
        shutil.rmtree(destination)
        for name in FORKED:
            source = sources / f"{name}-{version}"
            target = destination / name
            shutil.copytree(source, target, ignore=lambda d, names: [
                n for n in names if n in DROPPED_FILES or (Path(d) == source and n in DROPPED_DIRS)])
            manifest = target / "Cargo.toml"
            write(manifest, fork_manifest(name, read(manifest), version, fork))
    dx12 = destination / "wgpu-hal/src/dx12/mod.rs"
    text = read(dx12)
    if text.count(DX12_STRETCH) != 1:
        sys.exit("the DX12 swap chain description changed upstream; reapply the scaling change by hand")
    write(dx12, text.replace(DX12_STRETCH, DX12_FORK))
    workspace = root / "Cargo.toml"
    text, count = re.subn(r'^(wgpu = \{ package = "gpux-wgpu", path = "vendor/wgpu/wgpu", version = ")=[^"]+(" \})$',
                          rf"\g<1>={fork}\g<2>", read(workspace), flags=re.M)
    if count != 1:
        sys.exit("workspace wgpu dependency not found")
    write(workspace, text)
    print(f"vendor/wgpu now holds gpux-wgpu {fork}; port gpux and validate")


if __name__ == "__main__":
    main()
