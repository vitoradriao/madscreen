"""Deriva os formatos do bundle do PNG vetorial renderizado (requer Pillow)."""
from pathlib import Path
from PIL import Image

icons = Path(__file__).resolve().parents[2] / "apps/madscreen-app/src-tauri/icons"
source = Image.open(icons / "source.png").convert("RGBA")
for filename, size in [("32x32.png", 32), ("128x128.png", 128),
                       ("128x128@2x.png", 256), ("icon.png", 512)]:
    source.resize((size, size), Image.Resampling.LANCZOS).save(icons / filename)
source.save(icons / "icon.ico", sizes=[(size, size) for size in (16, 24, 32, 48, 64, 128, 256)])
source.save(icons / "icon.icns")
