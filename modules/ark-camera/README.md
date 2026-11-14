# Camera

`ark-camera` (`modules/ark-camera`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
depending on `ark-camera` (ui/docs/app.md, How an App Opts In).

A QR code scanned, a photo taken, a photo taken while eyes in Rust watch
the frames, or a photo chosen from the library: each shows the platform's own camera page and answers when it
closes, through a typed reply (`ScanReply`, `PhotoReply`) the platform
answers once — a scan as `Text` (none when the person backed out), a photo
as its JPEG's `Bytes`.

## API

```rust
ark_camera::scan_qr()      // -> ScanQr, output Option<String>, later
ark_camera::take_photo()   // -> TakePhoto, output Option<Vec<u8>>, later
ark_camera::watch(eyes)    // -> Watch, output Option<Vec<u8>>, later
trait Camera {
    fn scan_qr(&self, reply: Arc<ScanReply>);
    fn take_photo(&self, reply: Arc<PhotoReply>);
    fn watch(&self, eyes: Arc<Eyes>, reply: Arc<PhotoReply>);
    fn pick_photo(&self, reply: Arc<PhotoReply>);   // ark-scanner's PickPhoto, on the phones
}
// ScanReply.scanned(code) / PhotoReply.taken(jpeg), or .failed(why)
trait See { fn look(&self, frame: &Frame) -> Look; }   // Rust's eyes: Eyes::new(Arc<dyn See>)
struct Frame { width, height, luma, rgb /* SQUARE² RGB or empty */, turns, at_ms }
struct Look { outline /* 8 floats, upright, 0..1 */, held, words, take }
ark_camera::Fake // .scanned(code), .photographed(jpeg) (a photo taken or picked), .show(frame) -> Look, .watched(jpeg)
```

**Watched.** The camera's page hands each frame of its analysis stream to
the eyes it was given and draws what they answer: their outline, gliding
to where they last saw it on `Springs.move` (fading on `Springs.fade`),
drawn fuller once it has `held`; their `words` above; and the photo taken
when they say `take`, or at the shutter at once. The photo is handed back
as the sensor took it, unturned: the frames' `turns` stand both upright.
The camera knows nothing of what the eyes look for: ark-scanner's page
finder is such eyes (modules/ark-scanner).

## Per Platform

| | QR code | Photo | Watched | Picked |
|---|---|---|---|---|
| iPhone, iPad (`PhoneCamera`, swift/) | VisionKit's data scanner (ArkUI's `QrScanner`) | `UIImagePickerController` | AVFoundation: the back camera's 4:3 photo preset, its frames' luminance (about 640 wide) to the eyes, a JPEG from `AVCapturePhotoOutput`; the overlay in SwiftUI over ArkUI's parts | `PHPickerViewController`, one image, as a JPEG at most 3000 on its long side |
| Mac | fails, said: no camera to hold over a page | fails | fails | `NSOpenPanel` of images, as a JPEG |
| Android (`PhoneCamera`, kotlin/; its page `CameraPage`, put once at the app's root) | fails, said: no decoder yet (one is a new dependency, deps.json regenerated) | the watched page with no eyes | Camera2 (no library): a preview, a YUV stream about 640 wide whose luminance and a 256-pixel colour square go to the eyes on a thread of their own, a frame at a time, and the full JPEG | the system's Photo Picker (`PickVisualMedia`, no permission), stood upright by its EXIF, a JPEG at most 3000 on its long side |
| Desktop, browser | `Unavailable` | `Unavailable` | `Unavailable` (ark-scanner chooses a file there) | `Unavailable` (ark-scanner chooses a file there) |

An app's runtimes take what came of a page once it answers, with nothing
of the app's on the phone: the answer wakes the app's host (ark-app's
`Inbox::ask_telling`, ark-ffi's `Handle::moved`), which drains every
runtime on its own thread. The app library registers the camera as the
process's when the app's crate depends on it (module.toml's `[phone]`), and on Android
puts `CameraPage` at the root.

## Motion

The outline's glide is `Springs.move` (critically damped: nothing threw
it), its coming and going `Springs.fade`; the shutter is `.press` /
`pressable`; the flash on Android dies away on `Springs.size`, softer under
reduced motion. Not seen on a device.

## Permissions

Declared in `module.toml`, merged by `ui/prebuild.py` into the generated projects:

- iPhone, Mac: `NSCameraUsageDescription` (an app's own wording wins).
- Android: `android.permission.CAMERA` (checked to be in the app's manifest).
