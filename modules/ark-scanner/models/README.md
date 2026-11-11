# corners.onnx

Where a receipt is in a camera frame, for the scanner's eyes
(`../src/model.rs`, feature `model`): DocAligner's `lcnet100` heatmap model
(`lcnet100_h_e_bifpn_256_fp32.onnx`, sha256 `f4117b78…594e`, Apache-2.0,
<https://github.com/DocsaidLab/DocAligner>), as its package downloads it.

Its twenty BiFPN `Einsum('i,i...->...')` nodes, a weighted sum over a stack,
are written as `Unsqueeze`, `Mul` and `ReduceSum` by `rewrite.py`, because
tract (the runtime the phone links) does not translate that Einsum; the two
models agree to within 2e-7 on every output. The server runs the original
with ONNX Runtime (`../../server/ocr/package.nix`).

    python rewrite.py lcnet100_h_e_bifpn_256_fp32.onnx corners.onnx

Chosen over OpenCV's contours, the bright-region detector the scanner had and
DocCornerNet LEAN: on 200 generated scenes (dark wood, a white counter,
glare, clutter, shadow) it found the page 94% of the time, against 34%, 49%
and 63%, and it was right on the real receipts the others missed.
