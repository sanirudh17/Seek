"""Reproduce the Windows icon from the supplied transparent artwork (Pillow)."""
import argparse
import io
import struct
from pathlib import Path
from PIL import Image, ImageFilter, ImageOps

parser = argparse.ArgumentParser()
parser.add_argument("source", type=Path)
parser.add_argument("--output", type=Path, default=Path("app/assets/seek.ico"))
parser.add_argument("--proof", type=Path, default=Path(".references/app-icon-proof.png"))
args = parser.parse_args()
source = Image.open(args.source).convert("RGBA")
square = Image.new("RGBA", (max(source.size),) * 2)
square.alpha_composite(source, ((square.width-source.width)//2, (square.height-source.height)//2))
frames = []
for size in (16, 24, 32, 48, 64, 128, 256):
    # Premultiplied resizing avoids dark halos in partially transparent pixels.
    frame = square.convert("RGBa").resize((size, size), Image.Resampling.LANCZOS).convert("RGBA")
    if size <= 48:
        alpha = frame.getchannel("A")
        frame = frame.filter(ImageFilter.UnsharpMask(radius=0.55, percent=65, threshold=3))
        frame.putalpha(alpha)
    if size == 256:
        buffer = io.BytesIO()
        frame.save(buffer, format="PNG")
        data = buffer.getvalue()
    else:
        # Legacy-compatible 32-bit DIB plus one-bit transparency AND mask.
        pixels = frame.transpose(Image.Transpose.FLIP_TOP_BOTTOM).tobytes("raw", "BGRA")
        stride = ((size + 31)//32)*4
        mask = bytearray(stride*size)
        for y in range(size):
            for x in range(size):
                if frame.getpixel((x, size-1-y))[3] == 0:
                    mask[y*stride+x//8] |= 0x80 >> (x%8)
        data = struct.pack("<IiiHHIIiiII", 40, size, size*2, 1, 32, 0, len(pixels), 0, 0, 0, 0) + pixels + mask
    frames.append((size, frame, data))
offset = 6 + 16*len(frames)
directory = bytearray(struct.pack("<HHH", 0, 1, len(frames)))
for size, frame, data in frames:
    directory.extend(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset))
    offset += len(data)
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_bytes(directory + b"".join(data for _, _, data in frames))
proof = Image.new("RGB", (620, 220), "#eeeeee")
from PIL import ImageDraw
draw = ImageDraw.Draw(proof)
for row, bg in enumerate(("#202020", "#f3f3f3")):
    draw.rectangle((0, row*110, 620, (row+1)*110), fill=bg)
    for col, (size, frame, _) in enumerate(frames[:5]):
        x, y = 22+col*118, row*110+24
        proof.paste(frame, (x, y), frame)
        draw.text((x, row*110+91), f"{size}px", fill="#bbbbbb" if row == 0 else "#333333")
args.proof.parent.mkdir(parents=True, exist_ok=True)
proof.save(args.proof)
with Image.open(args.output) as ico:
    assert ico.ico.sizes() == {(n,n) for n, _, _ in frames}
print(f"Verified {len(frames)} RGBA frames; {args.output.stat().st_size:,} bytes")
