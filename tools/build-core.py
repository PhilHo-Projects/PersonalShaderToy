"""Build the browser copy of the shared shader compiler."""

import hashlib
import os
import shutil
import subprocess
from pathlib import Path
from compilers import ROOT, install


def fingerprint():
    h = hashlib.sha256()
    for p in [
        ROOT / "Cargo.lock",
        ROOT / "shader-core/Cargo.toml",
        *sorted((ROOT / "shader-core/src").glob("*.rs")),
    ]:
        h.update(p.read_bytes().replace(b"\r\n", b"\n"))
    return h.hexdigest()


if __name__ == "__main__":
    sdk = Path(os.environ.get("WASI_SDK_PATH", ROOT / "target/compiler-tools/wasi-sdk"))
    if not os.environ.get("WASI_SDK_PATH"):
        install("sdk", sdk)
    out = ROOT / "target/web-core"
    env = dict(
        os.environ,
        CXXFLAGS_wasm32_unknown_unknown="-fexceptions -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false",
        WASI_SDK_PATH=str(sdk),
        CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C link-arg=-zstack-size=16777216",
    )
    exe = shutil.which("wasm-pack.cmd") or shutil.which("wasm-pack")
    subprocess.run(
        [
            exe,
            "build",
            str(ROOT / "shader-core"),
            "--target",
            "web",
            "--release",
            "--no-opt",
            "--out-dir",
            str(out),
            "--",
            "--locked",
        ],
        env=env,
        check=True,
    )
    dest = ROOT / "apps/web/compiler"
    dest.mkdir(exist_ok=True)
    for name in ["shader_core.js", "shader_core_bg.wasm", "shader_core.d.ts"]:
        shutil.copyfile(out / name, dest / name)
    (dest / "source.sha256").write_text(fingerprint() + "\n")
