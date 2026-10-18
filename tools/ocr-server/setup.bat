@echo off
rem Set up the OCR server on Windows: OpenCV for the system's python, and the
rem text detection and text recognition networks it reads with, from the
rem OpenCV model zoo. Everything goes under .cache\ beside this script: pip
rem into .cache\pip, OpenCV and numpy into .cache\site, the networks into
rem .cache\models.
setlocal

set "cache=%~dp0.cache"
if not exist "%cache%" mkdir "%cache%"

curl -fsSL -o "%cache%\get-pip.py" https://bootstrap.pypa.io/get-pip.py || exit /b 1
python "%cache%\get-pip.py" --quiet --target "%cache%\pip" || exit /b 1
set "PYTHONPATH=%cache%\pip"
python -m pip install --quiet --target "%cache%\site" opencv-python-headless numpy || exit /b 1
set "PYTHONPATH="

set "zoo=https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models"
if not exist "%cache%\models" mkdir "%cache%\models"
curl -fsSL -o "%cache%\models\text_detection_en_ppocrv3_2023may.onnx" "%zoo%/text_detection_ppocr/text_detection_en_ppocrv3_2023may.onnx" || exit /b 1
curl -fsSL -o "%cache%\models\text_recognition_CRNN_EN_2021sep.onnx" "%zoo%/text_recognition_crnn/text_recognition_CRNN_EN_2021sep.onnx" || exit /b 1

echo ocr server: %cache%
