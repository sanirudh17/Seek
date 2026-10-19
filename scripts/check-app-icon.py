"""Read-only native resource/window icon verification. Writes owned proof files."""
import argparse
import ctypes as c
from ctypes import wintypes as w
from pathlib import Path
import json
from PIL import Image, ImageDraw

p = argparse.ArgumentParser()
p.add_argument("executable", type=Path)
p.add_argument("--pid", type=int)
p.add_argument("--output", type=Path, default=Path(".references/native-icon"))
a = p.parse_args()
u, g = c.WinDLL("user32", use_last_error=True), c.WinDLL("gdi32", use_last_error=True)
u.PrivateExtractIconsW.argtypes = [w.LPCWSTR, c.c_int, c.c_int, c.c_int, c.POINTER(w.HICON), c.POINTER(w.UINT), w.UINT, w.UINT]
u.PrivateExtractIconsW.restype = w.UINT
u.GetDC.argtypes = [w.HWND]; u.GetDC.restype = w.HDC
g.CreateCompatibleDC.argtypes = [w.HDC]; g.CreateCompatibleDC.restype = w.HDC
g.CreateCompatibleBitmap.argtypes = [w.HDC, c.c_int, c.c_int]; g.CreateCompatibleBitmap.restype = w.HBITMAP
g.SelectObject.argtypes = [w.HDC, w.HANDLE]; g.SelectObject.restype = w.HANDLE
g.CreateSolidBrush.argtypes = [w.DWORD]; g.CreateSolidBrush.restype = w.HBRUSH
u.FillRect.argtypes = [w.HDC, c.POINTER(w.RECT), w.HBRUSH]
u.DrawIconEx.argtypes = [w.HDC, c.c_int, c.c_int, w.HICON, c.c_int, c.c_int, w.UINT, w.HBRUSH, w.UINT]
g.GetDIBits.argtypes = [w.HDC, w.HBITMAP, w.UINT, w.UINT, c.c_void_p, c.c_void_p, w.UINT]
g.DeleteObject.argtypes = [w.HANDLE]; g.DeleteDC.argtypes = [w.HDC]
u.ReleaseDC.argtypes = [w.HWND, w.HDC]; u.DestroyIcon.argtypes = [w.HICON]

def render(icon, size, bg):
    screen = u.GetDC(None)
    dc = g.CreateCompatibleDC(screen)
    bitmap = g.CreateCompatibleBitmap(screen, size, size)
    old = g.SelectObject(dc, bitmap)
    brush = g.CreateSolidBrush(bg | bg << 8 | bg << 16)
    rect = w.RECT(0, 0, size, size)
    u.FillRect(dc, c.byref(rect), brush)
    assert u.DrawIconEx(dc, 0, 0, icon, size, size, 0, None, 3)
    g.SelectObject(dc, old)
    import struct
    header = c.create_string_buffer(struct.pack("<IiiHHIIiiII", 40, size, -size, 1, 32, 0, size*size*4, 0, 0, 0, 0))
    pixels = c.create_string_buffer(size*size*4)
    assert g.GetDIBits(dc, bitmap, 0, size, pixels, header, 0) == size
    image = Image.frombytes("RGB", (size,size), pixels.raw, "raw", "BGRX")
    g.DeleteObject(brush); g.DeleteObject(bitmap); g.DeleteDC(dc); u.ReleaseDC(None,screen)
    return image

a.output.parent.mkdir(parents=True, exist_ok=True)
proof = Image.new("RGB", (400,180))
draw = ImageDraw.Draw(proof)
checks = []
for row, bg in enumerate((32,243)):
    draw.rectangle((0,row*90,400,(row+1)*90), fill=(bg,)*3)
    for col, size in enumerate((16,32)):
        icon, resource = w.HICON(), w.UINT()
        count = u.PrivateExtractIconsW(str(a.executable.resolve()), 0, size, size, c.byref(icon), c.byref(resource), 1, 0)
        assert count == 1 and icon.value, (count,c.get_last_error())
        image = render(icon,size,bg)
        proof.paste(image,(24+col*170,row*90+12))
        draw.text((24+col*170,row*90+60),f"Windows {size}px", fill="#bbbbbb" if row==0 else "#333333")
        checks.append({"size":size,"background":bg,"resource_id":resource.value,"non_background_pixels":sum(rgb != (bg,)*3 for rgb in image.getdata())})
        u.DestroyIcon(icon)
proof.save(str(a.output)+".png")
if a.pid:
    u.GetWindowThreadProcessId.argtypes=[w.HWND,c.POINTER(w.DWORD)]
    u.SendMessageW.argtypes=[w.HWND,w.UINT,w.WPARAM,w.LPARAM]; u.SendMessageW.restype=w.LPARAM
    callback_type=c.WINFUNCTYPE(w.BOOL,w.HWND,w.LPARAM)
    def visit(hwnd,_):
        pid=w.DWORD();u.GetWindowThreadProcessId(hwnd,c.byref(pid))
        if pid.value == a.pid and u.IsWindowVisible(hwnd):
            icons={str(kind):u.SendMessageW(hwnd,0x7f,kind,0) for kind in (0,1)}
            assert all(icons.values()),icons
            checks.append({"window":hwnd,"small_and_large_icons_present":True})
        return True
    u.EnumWindows(callback_type(visit),0)
    assert any("window" in check for check in checks),"No visible Seek window"
Path(str(a.output)+".json").write_text(json.dumps(checks,indent=2),encoding="utf-8")
print(json.dumps(checks))
