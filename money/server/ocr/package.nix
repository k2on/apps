# receipt-ocr (./receipt_ocr.py): what OCR sees of a receipt's photo, for
# money-server to read the items from (services.money.ocr.package). RapidOCR
# runs PaddleOCR's PP-OCRv5 models on ONNX runtime, on the CPU; no language
# model reads anything. The models are fetched by their checksums, as
# RapidOCR's own list names them, rather than downloaded at run time.
{
  lib,
  python3,
  fetchurl,
  makeWrapper,
  runCommand,
}:
let
  base = "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.8.0/onnx";
  models = runCommand "receipt-ocr-models" { } ''
    mkdir -p $out
    ln -s ${
      fetchurl {
        url = "${base}/PP-OCRv5/det/ch_PP-OCRv5_det_mobile.onnx";
        sha256 = "4d97c44a20d30a81aad087d6a396b08f786c4635742afc391f6621f5c6ae78ae";
      }
    } $out/det.onnx
    ln -s ${
      fetchurl {
        url = "${base}/PP-OCRv5/rec/en_PP-OCRv5_rec_mobile.onnx";
        sha256 = "c3461add59bb4323ecba96a492ab75e06dda42467c9e3d0c18db5d1d21924be8";
      }
    } $out/rec.onnx
    ln -s ${
      fetchurl {
        url = "${base}/PP-OCRv4/cls/ch_ppocr_mobile_v2.0_cls_mobile.onnx";
        sha256 = "e47acedf663230f8863ff1ab0e64dd2d82b838fceb5957146dab185a89d6215c";
      }
    } $out/cls.onnx
  '';
  python = python3.withPackages (p: [
    p.rapidocr
    p.onnxruntime
    p.opencv4
  ]);
in
runCommand "receipt-ocr"
  {
    nativeBuildInputs = [ makeWrapper ];
    meta.mainProgram = "receipt-ocr";
    passthru = { inherit models; };
  }
  ''
    mkdir -p $out/bin $out/share/receipt-ocr
    cp ${./receipt_ocr.py} $out/share/receipt-ocr/receipt_ocr.py
    makeWrapper ${python}/bin/python3 $out/bin/receipt-ocr \
      --add-flags $out/share/receipt-ocr/receipt_ocr.py \
      --set RECEIPT_OCR_MODELS ${models}
  ''
