#!/usr/bin/env python3
"""Draws the Starfall art. Run from the example folder: python3 tools/make_assets.py

Everything is drawn four times too big and scaled down, which gives smooth edges.
"""
import math
import random
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

OUT = Path(__file__).resolve().parent.parent / "assets"
S = 4  # supersampling


def canvas(w, h):
    image = Image.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
    return image, ImageDraw.Draw(image)


def finish(image, name):
    w, h = image.size
    image.resize((w // S, h // S), Image.LANCZOS).save(OUT / name)
    print("wrote", name)


def scaled(points):
    return [(x * S, y * S) for x, y in points]


def ship():
    image, draw = canvas(64, 64)
    # Wings.
    draw.polygon(scaled([(32, 22), (60, 52), (46, 54), (32, 44), (18, 54), (4, 52)]), fill=(52, 96, 168, 255))
    draw.polygon(scaled([(32, 26), (52, 49), (44, 50), (32, 42), (20, 50), (12, 49)]), fill=(78, 134, 214, 255))
    # Body.
    draw.polygon(scaled([(32, 3), (42, 30), (40, 56), (24, 56), (22, 30)]), fill=(226, 232, 244, 255))
    draw.polygon(scaled([(32, 3), (37, 30), (36, 56), (32, 56)]), fill=(184, 194, 214, 255))
    # Cockpit.
    draw.ellipse([27 * S, 16 * S, 37 * S, 32 * S], fill=(40, 196, 220, 255))
    draw.ellipse([29 * S, 18 * S, 33 * S, 25 * S], fill=(170, 240, 250, 255))
    # Engine.
    draw.rectangle([26 * S, 55 * S, 38 * S, 60 * S], fill=(92, 100, 122, 255))
    finish(image, "ship.png")


def flame():
    w, h = 24, 44
    image = Image.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
    pixels = image.load()
    for y in range(h * S):
        for x in range(w * S):
            u = (x / (w * S)) * 2 - 1
            v = y / (h * S)
            width = (1 - v) ** 0.7 * 0.85  # wide at the top, a point at the bottom
            if width <= 0:
                continue
            d = abs(u) / width
            if d >= 1:
                continue
            core = max(0.0, 1 - d * 1.6) * (1 - v)
            alpha = (1 - d) ** 1.2 * (1 - v * 0.6)
            r = 255
            g = int(120 + 135 * core)
            b = int(30 + 200 * core * core)
            pixels[x, y] = (r, g, b, int(255 * min(1.0, alpha * 1.4)))
    finish(image, "flame.png")


def star():
    image, draw = canvas(56, 56)
    cx = cy = 28
    points = []
    for i in range(10):
        radius = 22 if i % 2 == 0 else 9.5
        angle = -math.pi / 2 + i * math.pi / 5
        points.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle)))
    glow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    ImageDraw.Draw(glow).polygon(scaled(points), fill=(255, 214, 64, 150))
    image.alpha_composite(glow.filter(ImageFilter.GaussianBlur(5 * S)))
    draw.polygon(scaled(points), fill=(255, 205, 56, 255))
    inner = [(cx + (x - cx) * 0.62, cy + (y - cy) * 0.62 - 1) for x, y in points]
    draw.polygon(scaled(inner), fill=(255, 236, 140, 255))
    finish(image, "star.png")


def rock():
    random.seed(4)
    image, draw = canvas(64, 64)
    cx = cy = 32
    points = []
    count = 11
    for i in range(count):
        angle = i * 2 * math.pi / count
        radius = random.uniform(21, 29)
        points.append((cx + radius * math.cos(angle), cy + radius * math.sin(angle)))
    draw.polygon(scaled(points), fill=(92, 84, 96, 255))
    lit = [(cx + (x - cx) * 0.86 - 2, cy + (y - cy) * 0.86 - 2.5) for x, y in points]
    draw.polygon(scaled(lit), fill=(128, 118, 130, 255))
    for x, y, r in [(24, 26, 5.5), (40, 36, 4), (30, 42, 3), (41, 22, 2.5)]:
        draw.ellipse([(x - r) * S, (y - r) * S, (x + r) * S, (y + r) * S], fill=(84, 76, 90, 255))
        draw.ellipse([(x - r + 1) * S, (y - r + 1.4) * S, (x + r) * S, (y + r) * S], fill=(108, 99, 112, 255))
    finish(image, "rock.png")


def dot():
    size = 16
    image = Image.new("RGBA", (size * S, size * S), (0, 0, 0, 0))
    pixels = image.load()
    for y in range(size * S):
        for x in range(size * S):
            d = math.hypot(x - size * S / 2 + 0.5, y - size * S / 2 + 0.5) / (size * S / 2)
            if d < 1:
                pixels[x, y] = (255, 255, 255, int(255 * (1 - d) ** 1.5))
    finish(image, "dot.png")


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    ship()
    flame()
    star()
    rock()
    dot()
