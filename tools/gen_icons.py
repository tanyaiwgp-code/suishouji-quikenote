# -*- coding: utf-8 -*-
"""随手记 · 应用图标生成器。
从 icon-master.png（960×960，画布导出 2x）生成 Tauri 所需的全套图标：
PNG 多尺寸 / icon.ico / icon.icns。运行：python gen_icons.py
"""
import os
from PIL import Image

BASE = os.path.dirname(os.path.abspath(__file__))
MASTER = os.path.join(BASE, "icon-master.png")
OUT = os.path.join(BASE, "..", "suishouji", "src-tauri", "icons")

master = Image.open(MASTER).convert("RGBA")

# 目标尺寸（与 Tauri 默认 icons/ 布局一致）
SQUARES = {
    "icon.png": 512,
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}

for name, size in SQUARES.items():
    img = master.resize((size, size), Image.LANCZOS)
    img.save(os.path.join(OUT, name), "PNG")
    print("png", name, size)

# ICO：多分辨率打包（任务栏 / 标题栏 / 资源管理器）
ico_sizes = [16, 24, 32, 48, 64, 128, 256]
master.save(
    os.path.join(OUT, "icon.ico"),
    format="ICO",
    sizes=[(s, s) for s in ico_sizes],
)
print("ico", ico_sizes)

# ICNS：macOS 用（保留，Windows 构建不消费）
icns_sizes = [16, 32, 64, 128, 256, 512]
try:
    master.save(
        os.path.join(OUT, "icon.icns"),
        format="ICNS",
        sizes=[(s, s) for s in icns_sizes] + [(s * 2, s * 2) for s in (16, 32, 128, 256)],
    )
    print("icns", icns_sizes)
except Exception as e:  # Pillow ICNS 写入受限时降级
    print("icns fallback:", e)
    master.save(os.path.join(OUT, "icon.icns"), format="ICNS")

print("done")
