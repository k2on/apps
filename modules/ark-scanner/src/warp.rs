//! A quad made a flat rectangle: the homography from the rectangle onto the
//! quad, and each of the rectangle's pixels sampled bilinearly where it
//! puts it.

use crate::vision::{Pt, Quad};

/// The homography taking the rectangle `w` by `h` (its corners clockwise
/// from 0,0) onto `q`: the eight coefficients h11..h32 (h33 is 1), from the
/// four corners' equations; none when the corners are in a line.
pub fn homography(q: &Quad, w: u32, h: u32) -> Option<[f64; 8]> {
    let (w, h) = (w as f32, h as f32);
    let src = [Pt::new(0.0, 0.0), Pt::new(w, 0.0), Pt::new(w, h), Pt::new(0.0, h)];
    let mut a = [[0f64; 9]; 8];
    for (i, (s, d)) in src.iter().zip(q.corners()).enumerate() {
        let (x, y, u, v) = (s.x as f64, s.y as f64, d.x as f64, d.y as f64);
        a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    solve(a)
}

/// Gaussian elimination with partial pivoting over an augmented 8×9.
fn solve(mut a: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for c in 0..8 {
        let p = (c..8).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        a.swap(c, p);
        let d = a[c][c];
        if d.abs() <= 1e-12 {
            return None;
        }
        for v in &mut a[c][c..] {
            *v /= d;
        }
        for r in 0..8 {
            let f = a[r][c];
            if r == c || f == 0.0 {
                continue;
            }
            let pivot = a[c];
            for (v, p) in a[r][c..].iter_mut().zip(&pivot[c..]) {
                *v -= f * p;
            }
        }
    }
    Some(std::array::from_fn(|i| a[i][8]))
}

/// Where the homography `m` takes (x, y).
pub fn map(m: &[f64; 8], x: f64, y: f64) -> (f64, f64) {
    let d = m[6] * x + m[7] * y + 1.0;
    ((m[0] * x + m[1] * y + m[2]) / d, (m[3] * x + m[4] * y + m[5]) / d)
}

/// The page in `src` (RGB, `sw` by `sh`) within `q`, flat: `w` by `h`
/// pixels, RGB, each sampled bilinearly where the homography puts it.
pub fn flatten(src: &[u8], sw: u32, sh: u32, q: &Quad, w: u32, h: u32) -> Option<Vec<u8>> {
    let m = homography(q, w, h)?;
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = map(&m, x as f64 + 0.5, y as f64 + 0.5);
            out.extend(sample(src, sw as usize, sh as usize, u - 0.5, v - 0.5));
        }
    }
    Some(out)
}

fn sample(src: &[u8], w: usize, h: usize, u: f64, v: f64) -> [u8; 3] {
    let x0 = (u.floor() as i64).clamp(0, w as i64 - 1) as usize;
    let y0 = (v.floor() as i64).clamp(0, h as i64 - 1) as usize;
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let fx = (u - x0 as f64).clamp(0.0, 1.0);
    let fy = (v - y0 as f64).clamp(0.0, 1.0);
    let px = |x: usize, y: usize, c: usize| src[(y * w + x) * 3 + c] as f64;
    std::array::from_fn(|c| {
        let top = px(x0, y0, c) * (1.0 - fx) + px(x1, y0, c) * fx;
        let bottom = px(x0, y1, c) * (1.0 - fx) + px(x1, y1, c) * fx;
        (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8
    })
}
