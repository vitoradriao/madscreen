"""Gera bitmaps NSIS com a marca monocromática existente (Python + Pillow)."""
from pathlib import Path
from PIL import Image, ImageFont, ImageDraw

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "apps/madscreen-app/src-tauri/windows/branding"
SCALE = 4
OUTPUT.mkdir(parents=True, exist_ok=True)
source = Image.open(ROOT / "apps/madscreen-app/src-tauri/icons/source.png").convert("RGBA")
# A marca branca (#fafafa) sobre o fundo #222c32 do ícone nativo.
# Recupera a cobertura suavizada do glifo, descartando o quadrado de fundo.
mask = source.getchannel("R").point(lambda value: max(0, min(255, round((value - 34) * 255 / 216))))
mask = mask.crop(mask.getbbox())


def paste_mark(canvas, xy, width, color):
    size = (width * SCALE, round(width * SCALE * mask.height / mask.width))
    coverage = mask.resize(size, Image.Resampling.LANCZOS)
    canvas.paste(color, (xy[0] * SCALE, xy[1] * SCALE), coverage)


sidebar = Image.new("RGB", (164 * SCALE, 314 * SCALE), "black")
paste_mark(sidebar, (22, 37), 70, "white")
header = Image.new("RGB", (150 * SCALE, 57 * SCALE), "black")

font = ImageFont.truetype(str(ROOT / "apps/madscreen-app/src-tauri/windows/fonts/OpenAISans-Regular.ttf"), 15 * SCALE)
ImageDraw.Draw(header).text((30 * SCALE, 17 * SCALE), "madscreen", font=font, fill="white")

for name, image, size in [("sidebar", sidebar, (164, 314)), ("header", header, (150, 57))]:
    image.save(OUTPUT / f"{name}.bmp")
    image.resize(size, Image.Resampling.LANCZOS).save(OUTPUT / f"{name}.png")
print("Bitmaps e prévias NSIS gerados em", OUTPUT)
