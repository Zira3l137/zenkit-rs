"""Builds the extension for this machine's platform, ready for Blender's "Install from Disk".

    python build.py

Needs Rust and maturin (`pip install maturin`). It builds the zenkit_py wheel, puts it into the
extension with the manifest saying which platform it is for, and zips the extension into dist/.
"""

import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ADDON = ROOT / "zenkit_io"
MANIFEST = ADDON / "blender_manifest.toml"


def platform_of(wheel):
    """Blender's name for the platform a wheel is for, like `windows-x64`."""
    tag = wheel.stem.split("-")[-1]
    system = (
        "windows" if tag.startswith("win") else "macos" if tag.startswith("macosx") else "linux"
    )
    arch = "arm64" if tag.endswith(("arm64", "aarch64")) else "x64"
    return f"{system}-{arch}"


def main():
    with tempfile.TemporaryDirectory() as out:
        subprocess.run(
            [sys.executable, "-m", "maturin", "build", "--release", "--out", out],
            cwd=ROOT,
            check=True,
        )
        (wheel,) = Path(out).glob("*.whl")
        wheels = ADDON / "wheels"
        shutil.rmtree(wheels, ignore_errors=True)
        wheels.mkdir()
        shutil.copy(wheel, wheels / wheel.name)

    platform = platform_of(wheel)
    manifest = MANIFEST.read_text(encoding="utf-8")
    built = f'platforms = ["{platform}"]\nwheels = ["./wheels/{wheel.name}"]\n\n'
    manifest = manifest.replace("[permissions]", built + "[permissions]")

    version = re.search(r'(?m)^version = "(.+)"$', manifest).group(1)
    dist = ROOT / "dist"
    dist.mkdir(exist_ok=True)
    archive = dist / f"zenkit_io-{version}-{platform}.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zip:
        for path in sorted(ADDON.rglob("*")):
            if path.is_file() and "__pycache__" not in path.parts and path != MANIFEST:
                zip.write(path, path.relative_to(ADDON))
        zip.writestr("blender_manifest.toml", manifest)
    print(f"built {archive}")


if __name__ == "__main__":
    main()
