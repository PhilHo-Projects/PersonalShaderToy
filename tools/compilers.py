"""Install pinned compiler distributions into build directories."""

import argparse
import hashlib
import os
import platform
import shutil
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SLANG = "2026.22"
SDK = "34.0"
ASSETS = {
    "native-windows-x86_64": (
        f"shader-slang/slang/releases/download/v{SLANG}/slang-{SLANG}-windows-x86_64.zip",
        "dd0c4a86bd44cdb7e9cf712af0c8ebf2fff1bba19fa5e3c128ff87384e8ff17b",
    ),
    "native-linux-x86_64": (
        f"shader-slang/slang/releases/download/v{SLANG}/slang-{SLANG}-linux-x86_64-glibc-2.28.tar.gz",
        "35c765d20b69f6217b02848cdb82a5216bf03990044a8991ceb0b89a198a80fb",
    ),
    "native-linux-arm64": (
        f"shader-slang/slang/releases/download/v{SLANG}/slang-{SLANG}-linux-aarch64-glibc-2.28.tar.gz",
        "08a9690c41a7624cf9f091ab1e5d5f02fa8a6891c57a635b5698386581e8e826",
    ),
    "native-darwin-arm64": (
        f"shader-slang/slang/releases/download/v{SLANG}/slang-{SLANG}-macos-aarch64.tar.gz",
        "885b5bd9a79a1ae81fdb92212f5f7c785001f870d45c39f9bb5df01339db3fea",
    ),
    "native-darwin-x86_64": (
        f"shader-slang/slang/releases/download/v{SLANG}/slang-{SLANG}-macos-x86_64.tar.gz",
        "baad48f7432c1826a84a396d9bd6a2ebc3c9ceb395a187baad49d29066fbcd02",
    ),
    "sdk-linux-arm64": (
        f"WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/wasi-sdk-{SDK}-arm64-linux.tar.gz",
        "f7e243dff54d60bcc576e94d6166b69f410f2500ae4a9ceef34315be10e77971",
    ),
    "sdk-darwin-arm64": (
        f"WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/wasi-sdk-{SDK}-arm64-macos.tar.gz",
        "9c59398106b417f8f14913380fdf0097a8cc0ff4af9eb3ce0065a859e88d49e9",
    ),
    "sdk-darwin-x86_64": (
        f"WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/wasi-sdk-{SDK}-x86_64-macos.tar.gz",
        "87d27fa8adc68dee59bfbf2e22a6d34ef717c34d6bf1d8af2a56fc929d9ce0eb",
    ),
    "web": (
        f"shader-slang/slang/releases/download/v{SLANG}/slang-{SLANG}-wasm.zip",
        "f5b2b1966225092aba6c0aa8e67cc20264be11281fd91d22e08801b1b7d75699",
    ),
    "sdk-windows-x86_64": (
        f"WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/wasi-sdk-{SDK}-x86_64-windows.tar.gz",
        "cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f",
    ),
    "sdk-linux-x86_64": (
        f"WebAssembly/wasi-sdk/releases/download/wasi-sdk-34/wasi-sdk-{SDK}-x86_64-linux.tar.gz",
        "b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4",
    ),
}


def install(kind, destination):
    dest = Path(destination).resolve()
    machine = platform.machine().lower()
    arch = {"amd64": "x86_64", "aarch64": "arm64"}.get(machine, machine)
    asset = kind if kind == "web" else f"{kind}-{platform.system().lower()}-{arch}"
    if asset not in ASSETS:
        raise RuntimeError(
            "Supply an installed compiler using PST_SLANGC or WASI_SDK_PATH on this platform."
        )
    path, digest = ASSETS[asset]
    stamp = dest / ".distribution"
    if stamp.is_file() and stamp.read_text().strip() == digest:
        return
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=dest.parent) as tmp:
        archive = Path(tmp) / path.split("/")[-1]
        with (
            urllib.request.urlopen("https://github.com/" + path, timeout=120) as source,
            archive.open("wb") as output,
        ):
            shutil.copyfileobj(source, output)
        with archive.open("rb") as source:
            if hashlib.file_digest(source, "sha256").hexdigest() != digest:
                raise ValueError(f"Checksum mismatch for {archive.name}")
        unpacked = Path(tmp) / "unpacked"
        unpacked.mkdir()
        if path.endswith(".zip"):
            with zipfile.ZipFile(archive) as z:
                z.extractall(unpacked)
        else:
            with tarfile.open(archive) as t:
                t.extractall(unpacked, filter="data")
        if kind == "sdk":
            source = next(unpacked.glob("wasi-sdk-*"))
        elif kind == "native":
            source = next(
                unpacked.rglob("slangc.exe" if os.name == "nt" else "slangc")
            ).parent.parent
        else:
            source = unpacked
        dest.mkdir(parents=True, exist_ok=True)
        shutil.copytree(source, dest, dirs_exist_ok=True, symlinks=True)
        stamp.write_text(digest)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["native", "sdk", "web"])
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    install(args.kind, args.destination)
