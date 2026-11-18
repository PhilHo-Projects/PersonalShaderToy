"""Exercise the built site and release desktop viewer on a real GPU.

Requires Python 3.11+, Pillow and Playwright with Chrome. Run from any directory.
"""

import argparse
import base64
import functools
import http.server
import io
import json
import os
import queue
import time
import subprocess
import tempfile
import threading
from pathlib import Path

from PIL import Image, ImageChops, ImageStat
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "target/viewer-checks"
FIXTURES = ROOT / "tests/fixtures"
EXAMPLES = {
    "cyberspace": "cyberspace_data_warehouse",
    "kerr-newman": "kerr_newman_black_hole",
}


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass


def compare(image, reference, name):
    # Filter subpixel star noise and retain the disk, shadow and large-scale color.
    a = image.convert("RGB").resize((160, 90), Image.Resampling.BOX)
    b = reference.convert("RGB").resize(a.size, Image.Resampling.BOX)
    error = sum(ImageStat.Stat(ImageChops.difference(a, b)).mean) / (3 * 255)
    print(f"{name} · low-frequency mean absolute error {error:.4f}", flush=True)
    tolerance = 0.015 if "kerr" in name.lower() else 0.055
    assert error < tolerance, (name, error)


def centered(image):
    w, h = image.size
    for band in (1, 2):
        box = image.getchannel(band).point(lambda v: 255 if v > 125 else 0).getbbox()
        assert (
            box and abs(box[0] + box[2] - w) <= 2 and abs(box[1] + box[3] - h) <= 2
        ), (image.size, box)


def browser():
    with (
        http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0),
            functools.partial(Quiet, directory=str(ROOT / "apps/web/dist")),
        ) as server,
        sync_playwright() as pw,
    ):
        threading.Thread(target=server.serve_forever, daemon=True).start()
        b = pw.chromium.launch(channel="chrome", headless=True)
        ctx = b.new_context(viewport={"width": 1250, "height": 820})
        page = ctx.new_page()
        requests = []
        page.on("request", lambda r: requests.append(r.url))
        page.add_init_script("""window.requestAnimationFrame=cb=>(window.nextShaderFrame=cb,1);
          const DateImpl=window.Date;window.Date=class extends DateImpl{constructor(...args){super(...(args.length?args:[2026,0,1,0,0,0]));}};
          Object.defineProperty(navigator,'clipboard',{value:{writeText:async text=>{window.copied=text}}});""")
        page.goto(
            f"http://127.0.0.1:{server.server_port}", wait_until="domcontentloaded"
        )

        def ready(title):
            page.wait_for_function(
                "t=>document.querySelector('#status').classList.contains('error') || (document.querySelector('#status').textContent==='Ready' && document.querySelector('#title').textContent===t)",
                arg=title,
                polling=80,
                timeout=65000,
            )
            assert page.locator("#status").inner_text() == "Ready", page.locator(
                "#status"
            ).inner_text()

        def choose(id, value):
            page.locator("#" + id).evaluate(
                "(e,v)=>{e.value=v;e.dispatchEvent(new Event('change'))}", value
            )

        def click(id):
            page.locator("#" + id).evaluate("e=>e.click()")

        def capture():
            data = page.evaluate("""()=>{for(let i=0;i<8;i++)window.nextShaderFrame(1000+i*1000/60);
              return document.querySelector('#stage canvas').toDataURL('image/png').split(',')[1];}""")
            return Image.open(io.BytesIO(base64.b64decode(data))).convert("RGB")

        def upload(name, source):
            page.locator("#file").set_input_files(
                {"name": name, "mimeType": "text/plain", "buffer": source.encode()}
            )

        ready("Cyberspace Data Warehouse")
        assert not any("/compilers/" in r for r in requests)
        assert page.locator("#library option").count() == 2
        choose("size", "720")
        for backend in ["webgl2", "webgpu"]:
            choose("backend", backend)
            for ident, name in EXAMPLES.items():
                choose("library", ident)
                ready(
                    "Cyberspace Data Warehouse"
                    if ident == "cyberspace"
                    else "Kerr–Newman Black Hole"
                )
                click("reset")
                image = capture()
                image.save(OUTPUT / f"{backend}-{name}.png")
                compare(
                    image,
                    Image.open(ROOT / f"tests/reference/{name}.png"),
                    backend + " " + name,
                )
        # Reopen the exact HLSL copied by the inspector. This exercises the heavy five-pass shader too.
        for ident, name in EXAMPLES.items():
            choose("library", ident)
            ready(
                "Cyberspace Data Warehouse"
                if ident == "cyberspace"
                else "Kerr–Newman Black Hole"
            )
            click("code")
            choose("source-type", "hlsl")
            page.wait_for_function(
                "()=>document.querySelector('#source').textContent.includes('SV_Target')",
                polling=80,
            )
            click("copy-all")
            hlsl = page.evaluate("window.copied")
            assert "SV_Target" in hlsl
            (OUTPUT / (ident + ".hlsl")).write_text(hlsl, encoding="utf-8")
            page.locator("#inspector [data-close]").evaluate("e=>e.click()")
            upload(ident + ".hlsl", hlsl)
            ready(ident + ".hlsl")
            image = capture()
            compare(
                image,
                Image.open(ROOT / f"tests/reference/{name}.png"),
                "Web HLSL " + ident,
            )
        choose("backend", "auto")
        for color, expected in [("1,0,0,1", (255, 0, 0)), ("0,1,0,1", (0, 255, 0))]:
            name = "color" + str(expected[0]) + ".hlsl"
            upload(
                name,
                "float4 main(float4 p:SV_Position):SV_Target0{return float4("
                + color
                + ");}",
            )
            ready(name)
            assert capture().getpixel((640, 360)) == expected
        # A selection with invalid code keeps the previous image and reports its original line.
        upload("bad.wgsl", "fn mainImage(p:vec2f)->vec4f{\nreturn missing;\n}")
        page.wait_for_function(
            "()=>document.querySelector('#status').classList.contains('error')",
            polling=50,
        )
        assert "line 2" in page.locator("#status").inner_text()
        assert page.locator("#title").inner_text() == name
        upload("centering.wgsl", (FIXTURES / "centering.wgsl").read_text())
        ready("centering.wgsl")
        original = capture()
        original.save(OUTPUT / "web-centering.png")
        centered(original)
        choose("scale", ".5")
        scaled = capture()
        assert scaled.size == (640, 360)
        centered(scaled)
        choose("scale", "1")
        click("code")
        choose("source-type", "hlsl")
        page.wait_for_function(
            "()=>document.querySelector('#source').textContent.includes('SV_Target')",
            polling=80,
            timeout=45000,
        )
        click("copy-all")
        hlsl = page.evaluate("window.copied")
        assert "//! PASS: Image" in hlsl
        (OUTPUT / "centering.hlsl").write_text(hlsl, encoding="utf-8")
        page.locator("#inspector [data-close]").evaluate("e=>e.click()")
        upload("roundtrip.hlsl", hlsl)
        ready("roundtrip.hlsl")
        compare(capture(), original, "Browser HLSL round trip")
        click("play")
        page.evaluate("window.nextShaderFrame(9000)")
        assert page.locator("#fps").inner_text() == "—"
        page.set_viewport_size({"width": 390, "height": 844})
        bounds = page.locator("#stage").bounding_box()
        assert bounds["x"] >= 0 and bounds["x"] + bounds["width"] <= 391
        page.screenshot(path=str(OUTPUT / "mobile.png"))
        print(
            "Web pause, validation, copy, import and mobile checks passed", flush=True
        )
        b.close()
        server.shutdown()


def native():
    exe = (
        ROOT
        / "target/release"
        / ("personal-shadertoy.exe" if os.name == "nt" else "personal-shadertoy")
    )

    def capture(path, backend, shader):
        args = [
            str(exe),
            "--backend",
            backend,
            "--open",
            str(shader),
            "--size",
            "1280x720",
            "--capture",
            str(path),
            "--frames",
            "8",
        ]
        completed = subprocess.run(
            args, cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=90
        )
        assert completed.returncode == 0, (
            completed.stdout[-2000:],
            completed.stderr[-1000:],
        )
        return Image.open(path).convert("RGB")

    for backend in ["dx12", "vulkan", "gl"]:
        for name in EXAMPLES.values():
            image = capture(
                OUTPUT / f"{backend}-{name}.png",
                backend,
                ROOT / f"shaders/Shadertoy/{name}.glsl",
            )
            compare(
                image,
                Image.open(ROOT / f"tests/reference/{name}.png"),
                backend + " " + name,
            )
    centering = capture(
        OUTPUT / "native-centering.png", "dx12", FIXTURES / "centering.wgsl"
    )
    if (OUTPUT / "centering.hlsl").exists():
        compare(
            capture(OUTPUT / "native-roundtrip.png", "dx12", OUTPUT / "centering.hlsl"),
            centering,
            "Native HLSL round trip",
        )

    for ident, name in EXAMPLES.items():
        source = OUTPUT / (ident + ".hlsl")
        if source.exists():
            image = capture(OUTPUT / ("native-" + ident + "-hlsl.png"), "dx12", source)
            compare(
                image,
                Image.open(ROOT / f"tests/reference/{name}.png"),
                "Native HLSL " + ident,
            )
    centered(centering)
    with tempfile.TemporaryDirectory() as directory:
        shader = Path(directory) / "watch.wgsl"
        solid = lambda color: f"fn mainImage(p:vec2f)->vec4f{{return vec4f({color});}}"
        shader.write_text(solid("1,0,0,1"))
        app = subprocess.Popen(
            [
                str(exe),
                "--backend",
                "dx12",
                "--size",
                "1280x720",
                "--open",
                str(shader),
            ],
            cwd=ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
        )
        events = queue.Queue()

        def read():
            for line in app.stdout:
                if line.startswith("{"):
                    events.put(json.loads(line))

        threading.Thread(target=read, daemon=True).start()

        def event(kind):
            deadline = time.monotonic() + 30
            while True:
                msg = events.get(timeout=max(0.001, deadline - time.monotonic()))
                if msg["type"] == kind:
                    return msg

        def command(kind, **fields):
            app.stdin.write(json.dumps({"type": kind, **fields}) + "\n")
            app.stdin.flush()

        def snap():
            command("take_screenshot")
            with Image.open(ROOT / event("ScreenshotTaken")["path"]) as captured:
                return captured.convert("RGB")

        try:
            event("FirstFrame")
            assert snap().getpixel((640, 360)) == (255, 0, 0)
            start = time.monotonic()
            shader.write_text(solid("0,1,0,1"))
            event("FirstFrame")
            elapsed = time.monotonic() - start
            print(
                f"Warm save to first rendered frame {elapsed * 1000:.0f} ms", flush=True
            )
            assert elapsed < 1.0
            assert snap().getpixel((640, 360)) == (0, 255, 0)
            shader.write_text("fn mainImage(p:vec2f)->vec4f{\nreturn missing;\n}")
            assert "line 2" in event("Diagnostic")["message"]
            assert snap().getpixel((640, 360)) == (0, 255, 0)
            temp = shader.with_suffix(".tmp")
            temp.write_text((FIXTURES / "centering.wgsl").read_text())
            temp.replace(shader)
            event("FirstFrame")
            centered(snap())
            command("set_render_scale", scale=0.5)
            image = snap()
            assert image.size == (640, 360)
            centered(image)
            for backend in ("gl", "dx12"):
                command("set_backend", backend=backend)
                event("FirstFrame")
                centered(snap())
            print(
                "Native atomic reload, last-good frame, scaling and GL → DX12 switch passed",
                flush=True,
            )
        finally:
            if app.poll() is None:
                command("shutdown")
                try:
                    app.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait()
    env = {
        **os.environ,
        "PST_AUTO_BENCH": "0.5,1",
        "PST_AUTO_BENCH_SHADER": str(FIXTURES / "centering.wgsl"),
    }
    bench = subprocess.run(
        [str(exe), "--size", "640x360"],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=100,
    )
    assert bench.returncode == 0, bench.stderr
    saved = next(
        json.loads(line)
        for line in bench.stdout.splitlines()
        if '"BenchmarkSaved"' in line
    )
    sweep = json.loads((ROOT / saved["path"]).read_text())
    runs = sweep["runs"]
    assert all(r["error"] is None and r["resolution"] == [640, 360] for r in runs), runs
    assert (
        len(runs) == 3
        and len({r["frames"] for r in runs}) == 1
        and runs[0]["frames"] > 1
    )
    assert all(r["frame_interval"]["count"] == r["frames"] for r in runs)
    print(
        "Benchmark matched",
        runs[0]["frames"],
        "measured frames per backend",
        flush=True,
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--only", choices=["web", "native"])
    args = parser.parse_args()
    OUTPUT.mkdir(exist_ok=True, parents=True)
    if args.only != "native":
        browser()
    if args.only != "web":
        native()
