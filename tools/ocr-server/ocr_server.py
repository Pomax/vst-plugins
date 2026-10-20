#!/usr/bin/env python3
"""Where text is in a picture, read with OpenCV's text detection and text
recognition networks, for the UI tests.

The window driver starts this once and keeps it running for the whole run, so
the networks are loaded once and not for every look at the screen.

A request is one line on standard input: the path of a picture, and after a
tab the text to look for, and after another tab `smallest` or `largest`. The
text and the size may be left out. The answer is one of these, then a line
that says only `end`:

- with no text: the picture's `width height`, then every region read, as
  `x y width height text`;
- with a text: the picture's `width height`, then `x y width height` for every
  place it was read, exact matches first;
- nothing before `end` when the text is nowhere in the picture;
- `error ` and what went wrong, when the picture could not be read.

Boxes are in the picture's pixels, from its top left. The recognition network
reads digits and the letters a to z, and no spaces, so both what is read and
what is looked for are compared lower case with everything else left out.
"""

import os
import sys
from pathlib import Path

os.environ["OPENCV_LOG_LEVEL"] = "ERROR"

import cv2 as cv
import numpy as np

HERE = Path(__file__).resolve().parent
MODELS = HERE / ".cache" / "models"
DETECTION = MODELS / "text_detection_en_ppocrv3_2023may.onnx"
RECOGNITION = MODELS / "text_recognition_CRNN_EN_2021sep.onnx"

# What the recognition network can answer with, in the order of its outputs
# after the first, which is the blank between characters.
CHARSET = "0123456789abcdefghijklmnopqrstuvwxyz"

# The detection network's input sides have to be multiples of this.
STRIDE = 32

# The mean colour the detection network was trained with, per channel.
DETECTION_MEAN = (122.67891434, 116.66876762, 104.00698793)

# How tall the recognition network's input is, and how wide at the least.
RECOGNITION_HEIGHT = 32
RECOGNITION_WIDTH = 100


def detector():
    model = cv.dnn_TextDetectionModel_DB(cv.dnn.readNet(str(DETECTION)))
    model.setBinaryThreshold(0.3)
    model.setPolygonThreshold(0.5)
    model.setMaxCandidates(500)
    model.setUnclipRatio(2.0)
    return model


def squeezed(text):
    """`text` as it is compared: lower case, and only what the recognition
    network can read."""
    return "".join(c for c in text.lower() if c in CHARSET)


def decoded(output):
    """The text in the recognition network's output: the likeliest
    character at each step, with blanks and repeats taken out."""
    steps = output.reshape(output.shape[0], -1)
    text = []
    previous = 0
    for step in steps:
        best = int(np.argmax(step))
        if best != 0 and best != previous:
            text.append(CHARSET[best - 1])
        previous = best
    return "".join(text)


def recognised(net, crop):
    """What a crop of the grey picture says. The crop is scaled to the
    network's height and as wide as that makes it, and to the network's
    least width when it will not take another."""
    height, width = crop.shape[:2]
    if height == 0 or width == 0:
        return ""
    wide = max(RECOGNITION_WIDTH, round(width * RECOGNITION_HEIGHT / height))
    for size in ((wide, RECOGNITION_HEIGHT), (RECOGNITION_WIDTH, RECOGNITION_HEIGHT)):
        blob = cv.dnn.blobFromImage(crop, scalefactor=1 / 127.5, size=size, mean=127.5)
        net.setInput(blob)
        try:
            return decoded(net.forward())
        except cv.error:
            continue
    return ""


def regions(detect, recognise, picture):
    """Every region of text in the picture, as its box in the picture's pixels
    and what it says."""
    height, width = picture.shape[:2]
    tall = -(-height // STRIDE) * STRIDE
    wide = -(-width // STRIDE) * STRIDE
    padded = cv.copyMakeBorder(picture, 0, tall - height, 0, wide - width, cv.BORDER_REPLICATE)
    detect.setInputParams(1.0 / 255.0, (wide, tall), DETECTION_MEAN)
    quads, _ = detect.detect(padded)

    grey = cv.cvtColor(picture, cv.COLOR_BGR2GRAY)
    found = []
    for quad in quads:
        x, y, w, h = cv.boundingRect(np.array(quad, dtype=np.int32))
        x, y = max(x, 0), max(y, 0)
        w, h = min(w, width - x), min(h, height - y)
        if w <= 0 or h <= 0:
            continue
        found.append(((x, y, w, h), recognised(recognise, grey[y : y + h, x : x + w])))
    for box in pluses(grey):
        found.append((box, "+"))
    # Top to bottom, then left to right, the order a page is read in.
    found.sort(key=lambda region: (region[0][1], region[0][0]))
    return found


# How large a `+` on a button is, in the picture's own pixels, at the least
# and at the most.
PLUS_SMALLEST = 5
PLUS_LARGEST = 24


def pluses(grey):
    """The box of every `+` in the grey picture: the recognition network has
    no `+` to answer with, and a button may carry nothing else. A `+` is a
    shape of its own, dark on light or light on dark, about as wide as it is
    tall, inked right across its middle row and right down its middle column,
    and clear in its four corners."""
    smallest, largest = PLUS_SMALLEST, PLUS_LARGEST
    found = []
    for flag in (cv.THRESH_BINARY_INV, cv.THRESH_BINARY):
        _, ink = cv.threshold(grey, 0, 255, flag | cv.THRESH_OTSU)
        count, labels, stats, _ = cv.connectedComponentsWithStats(ink, connectivity=8)
        for label in range(1, count):
            x, y, w, h = (int(v) for v in stats[label][:4])
            if not (smallest <= w <= largest and smallest <= h <= largest):
                continue
            if not 0.7 <= w / h <= 1.4:
                continue
            if looks_like_a_plus(labels[y : y + h, x : x + w] == label):
                found.append((x, y, w, h))
    return found


def looks_like_a_plus(shape):
    height, width = shape.shape
    middle_row = shape[height // 2 - 1 : height // 2 + 2, :].any(axis=0).mean()
    middle_column = shape[:, width // 2 - 1 : width // 2 + 2].any(axis=1).mean()
    across, down = max(1, width // 4), max(1, height // 4)
    corners = [
        shape[:down, :across],
        shape[:down, -across:],
        shape[-down:, :across],
        shape[-down:, -across:],
    ]
    clear = all(corner.mean() < 0.15 for corner in corners)
    return middle_row >= 0.8 and middle_column >= 0.8 and clear and shape.mean() < 0.5


def places(read, wanted, sized):
    """Every place `wanted` was read, exact matches first, or the one place
    with the smallest or largest lettering when `sized` asks for it. A region
    that holds more than the wanted text gives the part of its box the
    wanted text takes up, measured in characters."""
    # A `+` has nothing the recognition network reads, and is looked for as
    # itself.
    key = squeezed(wanted) or wanted.strip()
    if not key:
        return []
    allowed = len(key) // LETTERS_PER_MISREAD
    exact, containing = [], []
    for (x, y, w, h), text in read:
        if not text:
            continue
        if edits(key, text) <= allowed:
            exact.append((x, y, w, h))
            continue
        found = within(key, text, allowed)
        if found is None:
            continue
        start, end = found
        left = x + w * start // len(text)
        right = x + w * end // len(text)
        containing.append((left, y, max(1, right - left), h))
    if sized:
        among = exact or containing
        if not among:
            return []
        pick = min if sized == "smallest" else max
        return [pick(among, key=lambda box: box[3])]
    return exact + containing


# How many letters of what is looked for allow one letter misread, added or
# left out: "save preset" may be read as "saverpreset" and still be found.
LETTERS_PER_MISREAD = 5


def edits(a, b):
    """How many letters have to be changed, added or taken away to turn `a`
    into `b`."""
    previous = list(range(len(b) + 1))
    for i, letter in enumerate(a, 1):
        current = [i]
        for j, other in enumerate(b, 1):
            current.append(
                min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (letter != other))
            )
        previous = current
    return previous[-1]


def within(key, text, allowed):
    """Where in `text` the stretch is that `key` can be turned into with the
    fewest edits, at most `allowed`, as its start and end; or nothing."""
    previous = [0] * (len(text) + 1)
    previous_start = list(range(len(text) + 1))
    for i, letter in enumerate(key, 1):
        current = [i]
        current_start = [0]
        for j, other in enumerate(text, 1):
            choices = [
                (previous[j - 1] + (letter != other), previous_start[j - 1]),
                (previous[j] + 1, previous_start[j]),
                (current[j - 1] + 1, current_start[j - 1]),
            ]
            cost, start = min(choices)
            current.append(cost)
            current_start.append(start)
        previous, previous_start = current, current_start
    best = min(range(len(text) + 1), key=lambda end: previous[end])
    if previous[best] > allowed:
        return None
    return previous_start[best], best


def answer(detect, recognise, request):
    # Windows PowerShell's pipe writer puts a byte order mark in front of the
    # first request.
    parts = request.lstrip("﻿").rstrip("\r\n").split("\t")
    path = parts[0]
    wanted = parts[1] if len(parts) > 1 and parts[1] else None
    sized = parts[2] if len(parts) > 2 and parts[2] else None

    if not os.path.isfile(path):
        return [f"error there is no file {path!r}"]
    picture = cv.imread(path, cv.IMREAD_COLOR)
    if picture is None:
        return [f"error {path!r} is not a picture OpenCV can read"]
    height, width = picture.shape[:2]
    read = regions(detect, recognise, picture)

    if wanted is None:
        return [f"{width} {height}"] + [f"{x} {y} {w} {h} {text}" for (x, y, w, h), text in read]
    found = places(read, wanted, sized)
    if not found:
        return []
    return [f"{width} {height}"] + [f"{x} {y} {w} {h}" for x, y, w, h in found]


def main():
    for model in (DETECTION, RECOGNITION):
        if not model.exists():
            setup = HERE / ("setup.bat" if os.name == "nt" else "setup.sh")
            print(f"error {model} is not there; run {setup}", flush=True)
            print("end", flush=True)
            return
    detect = detector()
    recognise = cv.dnn.readNet(str(RECOGNITION))
    for request in sys.stdin:
        try:
            lines = answer(detect, recognise, request)
        except Exception as e:
            lines = [f"error {e}"]
        for line in lines:
            print(line)
        print("end", flush=True)


if __name__ == "__main__":
    main()
