# Camera

`ark-camera` (`modules/ark-camera`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `camera` in its `modules.toml` and depending on `ark-camera`.

A QR code scanned, a photo taken: each shows the platform's own camera
page and answers when it closes, through a typed reply (`ScanReply`, `PhotoReply`) the platform answers
once — a scan as `Text` (none when the person backed out), a photo as its
JPEG's `Bytes`.

## API

```rust
ark_camera::scan_qr()    // -> ScanQr, output Option<String>, later
ark_camera::take_photo() // -> TakePhoto, output Option<Vec<u8>>, later
trait Camera { fn scan_qr(&self, reply: Arc<ScanReply>); fn take_photo(&self, reply: Arc<PhotoReply>); }
// ScanReply.scanned(code) / PhotoReply.taken(jpeg), or .failed(why)
ark_camera::Fake // .scanned(code), .photographed(jpeg)
```

## Per Platform

No native implementation yet: money's join sheet keeps its hand-written
`QrScanner` until a screen that scans moves to Rust. The plan: VisionKit's
`DataScannerViewController` (iPhone), `AVCaptureSession` (Mac), CameraX
with a QR decoder (Android, a new dependency: deps.json regenerated). The
desktop and the browser answer `Unavailable`.

## Permissions

Declared in `module.toml`, merged by `ui/prebuild.py` into the generated projects:

- iPhone, Mac: `NSCameraUsageDescription` (an app's own wording wins).
- Android: `android.permission.CAMERA` (checked to be in the app's manifest).
