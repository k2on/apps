//! Where a page is in a camera frame, by DocAligner's corner model
//! (`models/README.md`) on tract, in the app's own library so the phone
//! needs no runtime of its own (feature `model`). The model gives four
//! heatmaps, one a corner; each corner is the middle of its largest blob
//! where the model is sure (0.3 and over), and a frame with a corner
//! nowhere is no page.

use std::sync::OnceLock;

use tract_onnx::prelude::*;

/// The side of the square the model looks at: ark-camera's colour square.
pub const SIDE: usize = ark_camera::SQUARE as usize;
/// The side of its heatmaps.
const HEAT: usize = 128;

type Model = TypedRunnableModel<TypedModel>;

fn model() -> Option<&'static Model> {
    static MODEL: OnceLock<Option<Model>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let bytes: &[u8] = include_bytes!("../models/corners.onnx");
            tract_onnx::onnx()
                .model_for_read(&mut &bytes[..])
                .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, SIDE, SIDE]).into()))
                .and_then(|m| m.into_optimized())
                .and_then(|m| m.into_runnable())
                .ok()
        })
        .as_ref()
}

/// A page's corners in a picture `SIDE` by `SIDE`, RGB, row by row:
/// clockwise from the top left, each x then y from 0 to 1 across the
/// picture; none when the model sees no page.
pub fn corners(rgb: &[u8]) -> Option<[f32; 8]> {
    if rgb.len() != SIDE * SIDE * 3 {
        return None;
    }
    // Blue, green, red, each 0 to 1, channels first: as it was trained.
    let x: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, SIDE, SIDE), |(_, c, y, x)| rgb[(y * SIDE + x) * 3 + (2 - c)] as f32 / 255.0).into();
    let out = model()?.run(tvec!(x.into())).ok()?;
    let heat = out[0].to_array_view::<f32>().ok()?;
    let mut corners = [0f32; 8];
    for c in 0..4 {
        let sure: Vec<bool> = (0..HEAT * HEAT).map(|i| heat[[0, c, i / HEAT, i % HEAT]] >= 0.3).collect();
        let (x, y) = middle_of_largest(&sure)?;
        corners[2 * c] = (x + 0.5) / HEAT as f32;
        corners[2 * c + 1] = (y + 0.5) / HEAT as f32;
    }
    Some(corners)
}

/// The middle of the largest four-connected blob of a `HEAT` square mask.
fn middle_of_largest(mask: &[bool]) -> Option<(f32, f32)> {
    let mut seen = vec![false; mask.len()];
    let mut best: Option<(usize, f32, f32)> = None;
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let (mut n, mut sx, mut sy, mut stack) = (0usize, 0.0f32, 0.0f32, vec![start]);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % HEAT, i / HEAT);
            (n, sx, sy) = (n + 1, sx + x as f32, sy + y as f32);
            let near = [
                (x > 0).then(|| i - 1),
                (x + 1 < HEAT).then(|| i + 1),
                (y > 0).then(|| i - HEAT),
                (y + 1 < HEAT).then(|| i + HEAT),
            ];
            for j in near.into_iter().flatten() {
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if best.is_none_or(|(m, _, _)| n > m) {
            best = Some((n, sx / n as f32, sy / n as f32));
        }
    }
    best.map(|(_, x, y)| (x, y))
}
