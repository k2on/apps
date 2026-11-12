# Scanner

`ark-scanner` (`modules/ark-scanner`): a device module (ui/docs/app.md, The
Device). An app opts in by depending on `ark-scanner`, which brings
`ark-camera`, whose frames and shutter it uses.

A page (a receipt, a letter) photographed and made flat, as a JPEG. The
port is the runtime's (`ark_module::scanner`), which an update asks
through: `Command::ask_then(scan_page(), Msg::Scanned)`.

## API

```rust
ark_scanner::scan_page()             // -> ScanPage, output Option<Vec<u8>> (the flat page's JPEG), later
trait Scanner { fn scan_page(&self, reply: Reply<Option<Vec<u8>>>); }
ark_scanner::over_camera(camera)     // the phones': the scanner over ark-camera's port
ark_scanner::Chosen                  // the desktop's (feature `desktop`): a file the window's chooser gives
ark_scanner::Browser                 // the browser's (feature `web`): the page's own file input
ark_scanner::fake::Fake              // .scanned(jpeg), .failed(why), .waiting()
// What it is made of, each tested:
ark_scanner::eyes::PageEyes          // ark_camera::See: the page found, held still and sharp, the picture taken once
ark_scanner::vision::{page, sharpness, Steady, Quad}   // the bright region, Otsu, the outline's quad
ark_scanner::warp::{homography, flatten}               // the quad made a flat rectangle
ark_scanner::page::flat(jpeg, seen, turns)             // the photo's page alone, flat and upright, as a JPEG
ark_scanner::model::corners(rgb)     // feature `model`: DocAligner's corner model on tract
```

## How a Scan Goes

1. The update asks `scan_page()`. On a phone the scanner asks the camera
   to `watch` with eyes of its own (`PageEyes`).
2. Each analysis frame: the page is found — by the corner model on the
   frame's colour square where it is built in (`model`), by the frame's
   largest bright region otherwise — and the frame's sharpness taken (the
   variance of its Laplacian; at least 60). The eyes answer its outline,
   stood upright, and a sentence ("Point the camera at the page", "Hold
   steady: it's blurry", "Hold still").
3. Held within 2% of the frame's diagonal for a second, and sharp, the
   eyes say `take` — once.
4. The photo comes back as the sensor took it. Where the page was last
   seen is scaled to it (when the photo is of the frame's proportions), the
   rectangle round it taken at full resolution (halved past 4000 pixels),
   flattened by the homography, turned upright and written as a JPEG; with
   no page seen, the whole photo (halved past 2600) turned upright.

## Per Platform

| | Implementation |
|---|---|
| iPhone, iPad | `over_camera`, over ark-camera's AVFoundation page; the bright region (the camera gives no colour square there yet) |
| Mac | `over_camera`: the camera fails there, said |
| Android | `over_camera`, over ark-camera's Camera2 page; the corner model (money's Android library links `model`) |
| Desktop (iced) | `Chosen`: the window opens its chooser (`arkui::files::choose`) and answers with the file |
| Browser | `Browser`: `<input type=file accept=image/* capture=environment>`, a phone's browser's camera or a computer's files |

The scanner has no Swift or Kotlin of its own: the frames and the shutter
are the camera module's.

## Permissions

None of its own (`module.toml`): the camera's.

## Verified

`cargo test -p ark-scanner` (the JVM tests of money's old Vision.kt, on
the same drawn frames, `java.util.Random(7)` and all: a page found where
it was drawn, upright and tilted twenty degrees; none without one, nor a
sliver; corners in order; a quad turned with its frame; steady only once
held; sharp text over blurred; a quad flattened onto its rectangle; and the
eyes and a whole scan over the camera's fake), and with `model` the corner
model on three drawn scenes (tests/model.rs). Not run on a phone's camera.
