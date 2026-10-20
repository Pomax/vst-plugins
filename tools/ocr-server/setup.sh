#!/usr/bin/env sh
# Set up the OCR server: OpenCV for the system's python3, and the text
# detection and text recognition networks it reads with, from the OpenCV model
# zoo. Everything goes under .cache/ beside this script: pip into .cache/pip,
# OpenCV and numpy into .cache/site, the networks into .cache/models.
set -e

here=$(cd "$(dirname "$0")" && pwd)
cache=$here/.cache

mkdir -p "$cache"
curl -fsSL -o "$cache/get-pip.py" https://bootstrap.pypa.io/get-pip.py
python3 "$cache/get-pip.py" --quiet --target "$cache/pip"
PYTHONPATH="$cache/pip" python3 -m pip install --quiet --target "$cache/site" \
    opencv-python-headless numpy

zoo=https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models
mkdir -p "$cache/models"
curl -fsSL -o "$cache/models/text_detection_en_ppocrv3_2023may.onnx" \
    "$zoo/text_detection_ppocr/text_detection_en_ppocrv3_2023may.onnx"
curl -fsSL -o "$cache/models/text_recognition_CRNN_EN_2021sep.onnx" \
    "$zoo/text_recognition_crnn/text_recognition_CRNN_EN_2021sep.onnx"

echo "ocr server: $cache"
