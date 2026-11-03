"""receipt-ocr IMAGE: what RapidOCR (PaddleOCR's PP-OCRv5 models, on ONNX
runtime) sees of a receipt's photo, as JSON on stdout, for money-server's
paper reader (koon-apps money/server/src/paper.rs) to read:

    {"readings": [[box, ...], [box, ...], [box, ...]]}

one reading for each size its text detector looks at the photo (736, 1280
and 1920 pixels), which see a receipt's small print differently. Each box
is {text, score, left, right, y, h, rx, rr}: y, rx and rr are its middle
and its edges with the page's tilt taken off. A box twice a line's height
holds two lines printed close, and is cut and read again, a line each.

First the page is found and made flat (`flat`): DocAligner's corner model
(lcnet100) says where its four corners are, and a photo of a receipt on a
table is warped to the paper alone. A photo already of the paper alone (a
phone's scanner flattened it) has its corners at the photo's own, and is
read as it is; so is one whose corners the model is not sure of.

RECEIPT_OCR_MODELS names the directory of the four models (./package.nix).
"""
import json, os, statistics, sys

import cv2
import numpy as np
import onnxruntime
from rapidocr import LangRec, ModelType, OCRVersion, RapidOCR

MODELS = os.environ["RECEIPT_OCR_MODELS"]
SIDES = (736, 1280, 1920)


def corners(picture):
    """The page's corners, clockwise from the top left, or None: each the
    middle of the largest blob of its heatmap where the model is sure."""
    s = onnxruntime.InferenceSession(f"{MODELS}/corners.onnx", providers=["CPUExecutionProvider"])
    h, w = picture.shape[:2]
    x = cv2.resize(picture, (256, 256)).astype(np.float32).transpose(2, 0, 1)[None] / 255.0
    heat = s.run(None, {"img": x})[0][0]
    pts = []
    for m in heat[:4]:
        n, _, st, mid = cv2.connectedComponentsWithStats((m >= 0.3).astype(np.uint8))
        if n < 2:
            return None
        i = 1 + st[1:, cv2.CC_STAT_AREA].argmax()
        pts.append((mid[i] + 0.5) * [w / m.shape[1], h / m.shape[0]])
    return np.array(pts, np.float32)


def flat(picture):
    """The paper alone, flattened; or the photo, when it already is or the
    corners found do not make a page."""
    q = corners(picture)
    if q is None:
        return picture
    h, w = picture.shape[:2]
    area = cv2.contourArea(q)
    if not cv2.isContourConvex(q.reshape(-1, 1, 2)) or area < 0.1 * w * h or area > 0.85 * w * h:
        return picture
    # A margin round it, so a corner found a little inside cuts nothing off.
    q = np.clip(q.mean(0) + (q - q.mean(0)) * 1.04, 0, [w - 1, h - 1]).astype(np.float32)
    tl, tr, br, bl = q
    pw = int(max(np.linalg.norm(tr - tl), np.linalg.norm(br - bl)))
    ph = int(max(np.linalg.norm(bl - tl), np.linalg.norm(br - tr)))
    to = np.float32([[0, 0], [pw - 1, 0], [pw - 1, ph - 1], [0, ph - 1]])
    return cv2.warpPerspective(picture, cv2.getPerspectiveTransform(q, to), (pw, ph), flags=cv2.INTER_CUBIC)


def engine(side):
    return RapidOCR(params={
        "Global.log_level": "error",
        "Det.model_path": f"{MODELS}/det.onnx", "Det.ocr_version": OCRVersion.PPOCRV5,
        "Det.limit_side_len": side,
        "Rec.model_path": f"{MODELS}/rec.onnx", "Rec.ocr_version": OCRVersion.PPOCRV5,
        "Rec.lang_type": LangRec.EN, "Rec.model_type": ModelType.MOBILE,
        "Cls.model_path": f"{MODELS}/cls.onnx",
    })


def reading(ocr, picture):
    r = ocr(picture)
    boxes = []
    none = lambda v: [] if v is None else v
    for box, text, score in zip(none(r.boxes), none(r.txts), none(r.scores)):
        xs = [p[0] for p in box]
        ys = [p[1] for p in box]
        (x0, y0), (x1, y1) = box[0], box[1]
        boxes.append({"text": text, "score": float(score), "left": float(min(xs)), "right": float(max(xs)),
                      "mid": float(sum(ys) / 4), "h": float(max(ys) - min(ys)), "top": float(min(ys)),
                      "slope": float((y1 - y0) / max(1.0, x1 - x0)), "w": float(x1 - x0),
                      # The text's own height, along the box's side: a tilted box is
                      # taller on the page than its line.
                      "th": float(((box[3][0] - box[0][0]) ** 2 + (box[3][1] - box[0][1]) ** 2) ** 0.5)})
    if not boxes:
        return []
    line_h = statistics.median(b["th"] for b in boxes)
    split = []
    for b in boxes:
        k = round(b["th"] / line_h)
        # Only a level box is cut along the page's rows.
        if k < 2 or b["th"] < 1.7 * line_h or abs(b["slope"]) > 0.05:
            split.append(b)
            continue
        strip = b["h"] / k
        for n in range(k):
            top = int(b["top"] + n * strip)
            crop = picture[max(0, top - 2):int(top + strip) + 2, max(0, int(b["left"]) - 2):int(b["right"]) + 2]
            got = ocr(crop, use_det=False, use_cls=False)
            if got.txts and got.txts[0].strip():
                split.append(dict(b, text=got.txts[0], score=float(got.scores[0]), mid=top + strip / 2, h=strip))
    long = [b["slope"] for b in split if b["w"] > 3 * b["h"]]
    slope = statistics.median(long) if long else 0.0
    out = []
    for b in split:
        out.append({"text": b["text"], "score": b["score"], "left": b["left"], "right": b["right"],
                    "y": b["mid"] - slope * b["left"], "h": b["h"],
                    "rx": b["left"] + slope * b["mid"], "rr": b["right"] + slope * b["mid"]})
    return out


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: receipt-ocr IMAGE")
    path = sys.argv[1]
    picture = cv2.imread(path)
    if picture is None:
        sys.exit(f"receipt-ocr: {path} is not an image this reads")
    if os.environ.get("RECEIPT_OCR_FLAT", "1") != "0":
        picture = flat(picture)
    json.dump({"readings": [reading(engine(side), picture) for side in SIDES]}, sys.stdout)


if __name__ == "__main__":
    main()
