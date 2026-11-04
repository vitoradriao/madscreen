"""Install exact public-header prerequisites without native Make command limits."""
from pathlib import Path
import json
import shlex
import shutil
import subprocess
import sys

root = Path(sys.argv[1]).resolve()
build = root / "ffmpeg-build"
manifest = json.loads((root / "sources.json").read_text(encoding="utf-8-sig"))
make = next(row["root"] for row in manifest["sources"] if row["name"] == "make")
process = subprocess.run(
    [str(root / make / "WinRel/gnumake.exe"), "SHELL=C:/Progra~1/Git/usr/bin/sh.exe",
     "V=1", "-n", "install-headers"],
    cwd=build, check=True, capture_output=True, text=True, encoding="utf-8",
)
copied = []
for line in process.stdout.splitlines():
    if not line.startswith("install -m 644 "):
        continue
    arguments = shlex.split(line)
    destination = Path(arguments[-1]).resolve()
    runtime = root / "runtime"
    if not (destination.is_relative_to(runtime / "include")
            or destination == runtime / "lib/pkgconfig"):
        raise RuntimeError("Unexpected header install destination")
    destination.mkdir(parents=True, exist_ok=True)
    for item in arguments[3:-1]:
        source = Path(item)
        if not source.is_absolute():
            source = build / source
        source = source.resolve()
        if not source.is_relative_to(root) or source.suffix not in (".h", ".pc"):
            raise RuntimeError("Unexpected header install source")
        shutil.copy2(source, destination / source.name)
        copied.append(str((destination / source.name).relative_to(runtime)))
if len(copied) < 100 or not (root / "runtime/include/libavutil/frame.h").is_file():
    raise RuntimeError("Incomplete public-header install")
(root / "installed-headers.json").write_text(
    json.dumps(copied, indent=2) + "\n", encoding="utf-8")
print(f"Installed {len(copied)} exact public headers/pkg-config files")
