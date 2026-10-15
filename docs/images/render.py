"""Renders the README images of nothing-toolbox (PNG, drawn at 2x).

    python3 render.py OUT_DIR IOSEVKA_REGULAR.ttf IOSEVKA_BOLD.ttf DOTO.ttf \
        STATUS.ansi EQ.ansi VIZ.ansi

Needs Pillow. Fonts: Iosevka (https://github.com/be5invis/Iosevka) for all text and the
terminal screenshots, Doto (https://github.com/oliverlalan/Doto, SIL OFL) for the title.
The *.ansi inputs are real terminal output: `nothing status` and `nothing eq advanced`
redirected to a file, and one frame of `nothing headtrack --viz` captured with
`tmux capture-pane -e -p`. Each file starts with its own prompt line.

Style: monochrome ink on paper (or paper on ink), hairline rules, uppercase mono labels,
dot-matrix only for display, and a single red LED as the one accent. Terminal shots use
the Ayu Dark palette.
"""
import math
import re
import sys

from PIL import Image, ImageDraw, ImageFont

OUT, IOS_R, IOS_B, DOTO, STATUS, EQ, VIZ = sys.argv[1:8]
K = 2  # supersampling: every logical pixel is K device pixels

# --- Shared design tokens (monochrome, one red) -----------------------------------------
THEMES = {
    "light": dict(bg="#ffffff", ink="#000000", muted="#585a5a", hair="#e5e7eb", off="#e9eaec", red="#d71921"),
    "dark": dict(bg="#000000", ink="#ffffff", muted="#9a9c9c", hair="#2a2a2a", off="#1c1c1c", red="#e5262e"),
}


def mono(size, bold=False):
    return ImageFont.truetype(IOS_B if bold else IOS_R, size * K)


def doto(size, weight=700, round_=100):
    f = ImageFont.truetype(DOTO, size * K)
    try:
        axes = {a["name"] if isinstance(a.get("name"), str) else a["name"].decode(): a for a in f.get_variation_axes()}
        values = []
        for a in f.get_variation_axes():
            name = a["name"].decode() if isinstance(a["name"], bytes) else a["name"]
            values.append(weight if "eight" in name else round_)
        f.set_variation_by_axes(values)
    except OSError:
        pass
    return f


def spaced(draw, xy, text, font, fill, tracking=0.0):
    """Draws text with extra letter spacing (in logical px)."""
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=font, fill=fill)
        x += draw.textlength(ch, font=font) + tracking * K
    return x


def text_width(draw, text, font, tracking=0.0):
    return sum(draw.textlength(c, font=font) for c in text) + tracking * K * max(len(text) - 1, 0)


# --- Dot-matrix headphone ---------------------------------------------------------------
def headphone_cells(cols, rows):
    """Which LEDs are lit, decided at each LED centre: symmetric, no anti-aliasing.

    Designed on a 38-column reference grid; the cup centres sit on LED centres so that
    each cup has a single centre LED.
    """
    sc = cols / 38.0
    cx, cy, r = 19.0, 15.0, 14.5
    lit, centres = set(), []
    cup_w, cup_h, cup_r, cup_cy = 7.0, 11.0, 2.4, 24.5
    for j in range(rows):
        for i in range(cols):
            x, y = (i + 0.5) / sc, (j + 0.5) / sc
            on = False
            # headband: a ring, upper half
            dist = math.hypot(x - cx, y - cy)
            if y <= cy and r - 0.75 <= dist <= r + 0.75:
                on = True
            for ex in (cx - r, cx + r):
                # arm: one column from the band down to the cup
                if abs(x - ex) < 0.5 and cy <= y <= cup_cy - cup_h / 2:
                    on = True
                # cup: rounded rectangle with a round window and a centre LED
                dx, dy = abs(x - ex) - (cup_w / 2 - cup_r), abs(y - cup_cy) - (cup_h / 2 - cup_r)
                inside = math.hypot(max(dx, 0), max(dy, 0)) <= cup_r if (dx > 0 and dy > 0) else (
                    abs(x - ex) <= cup_w / 2 and abs(y - cup_cy) <= cup_h / 2)
                window = math.hypot(x - ex, y - cup_cy) <= 2.2
                if inside and not window:
                    on = True
                if math.hypot(x - ex, y - cup_cy) < 0.5:
                    on = True
            if on:
                lit.add((i, j))
    for ex in (cx - r, cx + r):
        centres.append((int(ex * sc), int(cup_cy * sc)))
    return lit, centres


def dot_matrix(img, x0, y0, cols, rows, pitch, theme, red=False):
    t = THEMES[theme]
    d = ImageDraw.Draw(img)
    lit, centres = headphone_cells(cols, rows)
    red_cell = centres[1] if red else None
    for j in range(rows):
        for i in range(cols):
            cx = (x0 + i * pitch + pitch / 2) * K
            cy = (y0 + j * pitch + pitch / 2) * K
            if (i, j) == red_cell:
                r, color = pitch * 0.36 * K, t["red"]
            elif (i, j) in lit:
                r, color = pitch * 0.36 * K, t["ink"]
            else:
                r, color = pitch * 0.11 * K, t["off"]
            d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)


# --- Hero -----------------------------------------------------------------------------------
def hero(theme):
    t = THEMES[theme]
    W, H = 960, 400
    img = Image.new("RGB", (W * K, H * K), t["bg"])
    d = ImageDraw.Draw(img)

    # outer hairline frame, square corners
    d.rectangle([0, 0, W * K - 1, H * K - 1], outline=t["hair"], width=K)

    # LED matrix with the headphone
    pitch, cols, rows = 10, 38, 32
    gx, gy = 40, 40
    dot_matrix(img, gx, gy, cols, rows, pitch, theme, red=True)

    # right column
    x = 470
    cap = mono(11)
    # chip
    chip = "UNOFFICIAL  ·  LINUX CLIENT"
    cwid = text_width(d, chip, cap, 0.6) / K
    d.rectangle([x * K, 58 * K, (x + cwid + 20) * K, 82 * K], outline=t["hair"], width=K)
    spaced(d, ((x + 10) * K, 63 * K), chip, cap, t["muted"], 0.6)

    size = 54
    while size > 20 and text_width(d, "nothing-toolbox", doto(size), 1.0) / K > W - 40 - x:
        size -= 1
    spaced(d, (x * K, 110 * K), "nothing-toolbox", doto(size), t["ink"], 1.0)

    body = mono(17)
    d.text((x * K, 190 * K), "Control the Nothing Headphone (1)", font=body, fill=t["ink"])
    d.text((x * K, 214 * K), "from a Linux terminal.", font=body, fill=t["ink"])

    # spec rows, hairline separated
    specs = [("MODEL", "B170"), ("LINK", "RFCOMM 15"), ("BUILT WITH", "RUST")]
    y = 270
    lab, val = mono(11), mono(13)
    for i, (k, v) in enumerate(specs):
        d.line([(x * K, y * K), ((W - 40) * K, y * K)], fill=t["hair"], width=K)
        spaced(d, (x * K, (y + 10) * K), k, lab, t["muted"], 0.6)
        d.text(((x + 170) * K, (y + 8) * K), v, font=val, fill=t["ink"])
        y += 34
    d.line([(x * K, y * K), ((W - 40) * K, y * K)], fill=t["hair"], width=K)

    img.save(f"{OUT}/hero-{theme}.png", optimize=True)


# --- Architecture -------------------------------------------------------------------------
def dotted(d, x0, y0, x1, y1, color, pitch=7, r=1.6):
    n = max(1, int(math.hypot(x1 - x0, y1 - y0) / pitch))
    for i in range(n + 1):
        x = (x0 + (x1 - x0) * i / n) * K
        y = (y0 + (y1 - y0) * i / n) * K
        d.ellipse([x - r * K, y - r * K, x + r * K, y + r * K], fill=color)


def card(d, x, y, w, h, t, strong=False):
    d.rounded_rectangle([x * K, y * K, (x + w) * K, (y + h) * K], radius=8 * K, fill=t["bg"],
                        outline=t["ink"] if strong else t["hair"], width=K)


def architecture(theme):
    t = THEMES[theme]
    W, H = 960, 430
    img = Image.new("RGB", (W * K, H * K), t["bg"])
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, W * K - 1, H * K - 1], outline=t["hair"], width=K)
    lab, code, small, num = mono(11), mono(14), mono(12), mono(11)

    def label(x, y, s, color=None):
        spaced(d, (x * K, y * K), s, lab, color or t["muted"], 0.6)

    # column headers with index numbers
    for x, n, s in [(40, "01", "SHORT COMMANDS"), (360, "02", "CHANNEL OWNER"), (690, "03", "HEADSET")]:
        label(x, 36, n, t["ink"])
        label(x + 30, 36, s)

    # 01 clients
    clients = [("nothing anc toggle", "BOUND TO A KEY"), ("nothing status", "BARS, SCRIPTS"), ("nothing eq set", "ANY OTHER COMMAND")]
    for i, (c, s) in enumerate(clients):
        y = 66 + i * 76
        card(d, 40, y, 250, 60, t)
        d.text((56 * K, (y + 12) * K), c, font=code, fill=t["ink"])
        label(56, y + 36, s)

    # 02 owner
    card(d, 360, 66, 260, 212, t, strong=True)
    owners = ["nothing daemon", "nothing watch", "nothing headtrack", "nothing gestures"]
    for i, o in enumerate(owners):
        d.text((378 * K, (84 + i * 26) * K), o, font=code, fill=t["ink"])
    d.line([(378 * K, 196 * K), (602 * K, 196 * K)], fill=t["hair"], width=K)
    label(378, 208, "ONE PROCESS HOLDS THE LINK")
    label(378, 226, "FANS EVENTS OUT")
    label(378, 244, "COUNTS HEAD-TRACKING USERS")

    # 03 headset, with a small LED matrix of the headphone
    card(d, 690, 66, 230, 212, t)
    dot_matrix(img, 718, 74, 38, 32, 4.6, theme)
    label(706, 230, "RFCOMM CHANNEL 15")
    # the single connection marker: one red dot
    rx, ry = 706, 252
    d.ellipse([(rx) * K, (ry + 1) * K, (rx + 8) * K, (ry + 9) * K], fill=t["red"])
    label(rx + 16, ry, "ONE CONNECTION AT A TIME")

    # links: clients -> owner (through the socket), owner <-> headset
    for i in range(3):
        y = 96 + i * 76
        dotted(d, 296, y, 352, 172 - 0 * i if i == 1 else (120 if i == 0 else 224), t["ink"])
    dotted(d, 626, 172, 684, 172, t["ink"], pitch=6, r=2.2)
    code_s = mono(12)
    d.text((360 * K, 292 * K), "via $XDG_RUNTIME_DIR/nothing.sock", font=code_s, fill=t["muted"])

    # fallback path when nobody owns the channel
    y = 360
    dotted(d, 165, 286, 165, y, t["muted"], pitch=8, r=1.3)
    dotted(d, 165, y, 805, y, t["muted"], pitch=8, r=1.3)
    dotted(d, 805, y, 805, 284, t["muted"], pitch=8, r=1.3)
    label(40, 384, "NO OWNER RUNNING  →  THE COMMAND OPENS THE CHANNEL ITSELF, FOR A FEW HUNDRED MS")

    img.save(f"{OUT}/architecture-{theme}.png", optimize=True)


# --- Terminal screenshots, kitty-like -------------------------------------------------------
AYU = {
    "bg": "#0b0e14", "fg": "#bfbdb6",
    "pal": ["#01060e", "#ea6c73", "#91b362", "#f9af4f", "#53bdfa", "#fae994", "#90e1c6", "#c7c7c7",
            "#686868", "#f07178", "#aad94c", "#ffb454", "#59c2ff", "#ffee99", "#95e6cb", "#ffffff"],
}
SGR = re.compile(r"\x1b\[([0-9;]*)m")
OTHER = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")


def parse_ansi(text):
    """Returns lines of (char, color, bold)."""
    lines = []
    for raw in text.split("\n"):
        out, color, bold, pos = [], AYU["fg"], False, 0
        raw = OTHER.sub(lambda m: m.group(0) if m.group(0).endswith("m") else "", raw)
        for m in SGR.finditer(raw):
            out += [(c, color, bold) for c in raw[pos:m.start()]]
            pos = m.end()
            codes = [int(c) for c in m.group(1).split(";") if c] or [0]
            i = 0
            while i < len(codes):
                c = codes[i]
                if c == 0:
                    color, bold = AYU["fg"], False
                elif c == 1:
                    bold = True
                elif c == 22:
                    bold = False
                elif c == 39:
                    color = AYU["fg"]
                elif 30 <= c <= 37:
                    color = AYU["pal"][c - 30]
                elif 90 <= c <= 97:
                    color = AYU["pal"][c - 90 + 8]
                elif c == 38 and i + 2 < len(codes) and codes[i + 1] == 5:
                    color = AYU["pal"][codes[i + 2]] if codes[i + 2] < 16 else AYU["fg"]
                    i += 2
                elif c == 38 and i + 4 < len(codes) and codes[i + 1] == 2:
                    color = "#%02x%02x%02x" % tuple(codes[i + 2:i + 5])
                    i += 4
                i += 1
        out += [(c, color, bold) for c in raw[pos:]]
        lines.append(out)
    while lines and not any(ch.strip() for ch, _, _ in lines[-1]):
        lines.pop()
    return lines


def terminal(src, name, cols=None):
    text = open(src, encoding="utf-8").read()
    lines = parse_ansi(text)
    size = 15
    reg, bld = mono(size), mono(size, bold=True)
    cell = reg.getlength("M") / K  # Iosevka: exactly 0.5 em
    lh = round(size * 1.3)
    ncols = cols or max(len(l) for l in lines)
    pad_x, pad_y = 26, 22
    W = int(pad_x * 2 + ncols * cell)
    H = int(pad_y * 2 + len(lines) * lh)
    img = Image.new("RGB", (W * K, H * K), AYU["bg"])
    d = ImageDraw.Draw(img)
    for row, line in enumerate(lines):
        for col, (ch, color, bold) in enumerate(line):
            if ch == " ":
                continue
            d.text(((pad_x + col * cell) * K, (pad_y + row * lh) * K), ch, font=bld if bold else reg, fill=color)
    img.save(f"{OUT}/{name}.png", optimize=True)


for th in THEMES:
    hero(th)
    architecture(th)
terminal(STATUS, "status")
terminal(EQ, "eq")
terminal(VIZ, "headtrack")
print("ok")
