//! Finding a page in a camera frame, over its luminance alone: the frame
//! shrunk to about [`LOOK`] wide and blurred, split at Otsu's threshold,
//! its largest bright region taken, and the four corners of that region's
//! outline found as a quad (the extremes of x+y and x−y, then each corner
//! moved along the outline to where the quad covers most). A region too
//! small, too little brighter than what it lies on, a quad that is not
//! convex, too thin, or covering too little of its region is no page.
//!
//! What the eyes fall back on where the corner model ([`crate::model`]) is
//! not built in, or has not answered yet; and how sharp a frame is
//! ([`sharpness`]).

/// A point in a frame's pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

impl Pt {
    pub const fn new(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    pub fn dist(self, o: Pt) -> f32 {
        (self.x - o.x).hypot(self.y - o.y)
    }

    fn times(self, s: f32) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
}

/// A page's corners, clockwise from the top left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quad {
    pub tl: Pt,
    pub tr: Pt,
    pub br: Pt,
    pub bl: Pt,
}

impl Quad {
    pub fn corners(&self) -> [Pt; 4] {
        [self.tl, self.tr, self.br, self.bl]
    }

    fn of(c: [Pt; 4]) -> Quad {
        Quad {
            tl: c[0],
            tr: c[1],
            br: c[2],
            bl: c[3],
        }
    }

    /// Its area (the shoelace), positive.
    pub fn area(&self) -> f32 {
        let c = self.corners();
        let s: f32 = (0..4).map(|i| c[i].x * c[(i + 1) % 4].y - c[(i + 1) % 4].x * c[i].y).sum();
        s.abs() / 2.0
    }

    /// Every turn the same way: no corner folded in.
    pub fn convex(&self) -> bool {
        let c = self.corners();
        let mut sign = 0;
        for i in 0..4 {
            let (a, b, d) = (c[i], c[(i + 1) % 4], c[(i + 2) % 4]);
            let z = (b.x - a.x) * (d.y - b.y) - (b.y - a.y) * (d.x - b.x);
            let s = if z > 0.0 {
                1
            } else if z < 0.0 {
                -1
            } else {
                return false;
            };
            if sign == 0 {
                sign = s
            } else if s != sign {
                return false;
            }
        }
        true
    }

    pub fn scaled(&self, s: f32) -> Quad {
        Quad::of(self.corners().map(|p| p.times(s)))
    }

    /// Moved by `dx`, `dy`: in a frame cut from this one at `-dx`, `-dy`.
    pub fn shifted(&self, dx: f32, dy: f32) -> Quad {
        Quad::of(self.corners().map(|p| Pt::new(p.x + dx, p.y + dy)))
    }

    /// The farthest any corner is from `o`'s.
    pub fn moved(&self, o: &Quad) -> f32 {
        self.corners().iter().zip(o.corners()).map(|(a, b)| a.dist(b)).fold(0.0, f32::max)
    }

    /// The page's size flat: its opposite sides' longer lengths.
    pub fn flat_size(&self) -> (u32, u32) {
        let w = self.tl.dist(self.tr).max(self.bl.dist(self.br));
        let h = self.tl.dist(self.bl).max(self.tr.dist(self.br));
        ((w.round() as u32).max(1), (h.round() as u32).max(1))
    }

    /// Turned with its frame a quarter `turns` times clockwise, the frame
    /// `w` by `h` before the turn.
    pub fn rotated(&self, turns: u8, w: u32, h: u32) -> Quad {
        let mut q = *self;
        let (mut fw, mut fh) = (w as f32, h as f32);
        for _ in 0..turns % 4 {
            let r = |p: Pt| Pt::new(fh - p.y, p.x);
            // Clockwise: the old bottom left becomes the new top left.
            q = Quad::of([r(q.bl), r(q.tl), r(q.tr), r(q.br)]);
            std::mem::swap(&mut fw, &mut fh);
        }
        q
    }

    /// Four points as a quad, clockwise from the top left (the least x+y).
    pub fn ordered(ps: [Pt; 4]) -> Quad {
        let cx = ps.iter().map(|p| p.x).sum::<f32>() / 4.0;
        let cy = ps.iter().map(|p| p.y).sum::<f32>() / 4.0;
        let mut round = ps;
        // Clockwise in image coordinates (y down): increasing angle.
        round.sort_by(|a, b| (a.y - cy).atan2(a.x - cx).total_cmp(&(b.y - cy).atan2(b.x - cx)));
        let start = (0..4)
            .min_by(|&a, &b| (round[a].x + round[a].y).total_cmp(&(round[b].x + round[b].y)))
            .unwrap_or(0);
        Quad::of(std::array::from_fn(|i| round[(start + i) % 4]))
    }
}

/// A frame's luminance: `w` by `h`, a byte a pixel, rows `stride` apart.
#[derive(Clone, Copy, Debug)]
pub struct Luma<'a> {
    pub w: usize,
    pub h: usize,
    pub px: &'a [u8],
    pub stride: usize,
}

impl<'a> Luma<'a> {
    /// `px` as `w` by `h`, rows packed; none when it is not that long.
    pub fn new(w: usize, h: usize, px: &'a [u8]) -> Option<Luma<'a>> {
        (px.len() >= w * h).then_some(Luma { w, h, px, stride: w })
    }

    pub fn at(&self, x: usize, y: usize) -> i32 {
        self.px[y * self.stride + x] as i32
    }
}

/// About how wide a frame is looked at for its page.
pub const LOOK: usize = 160;
/// The least of the frame a page takes.
pub const LEAST: f32 = 0.08;
/// How much brighter paper is than what it lies on, at the least, in levels of 255.
pub const CONTRAST: i32 = 45;

/// The page in `frame`, in its pixels, or none: the largest bright
/// region's corners, when they make a page.
pub fn page(frame: Luma) -> Option<Quad> {
    let step = (frame.w / LOOK).max(1);
    let (w, h) = (frame.w / step, frame.h / step);
    if w < 8 || h < 8 {
        return None;
    }
    let mut small = vec![0i32; w * h];
    for y in 0..h {
        for x in 0..w {
            small[y * w + x] = frame.at(x * step, y * step);
        }
    }
    let blurred = blur(&blur(&small, w, h), w, h);
    let cut = otsu(&blurred);
    let region = largest(&blurred, w, h, cut)?;
    if (region.len() as f32) < LEAST * (w * h) as f32 {
        return None;
    }
    if contrast(&blurred, &region) < CONTRAST {
        return None;
    }
    let q = corners(&outline(&region, w))?;
    if !q.convex() || q.area() < 0.85 * region.len() as f32 {
        return None;
    }
    let (fw, fh) = q.flat_size();
    if (fw.min(fh) as f32) < 0.12 * fw.max(fh) as f32 {
        return None;
    }
    // The small frame's pixel centres, back in the frame's.
    let s = step as f32;
    Some(Quad::of(q.corners().map(|p| Pt::new(p.x * s + s / 2.0, p.y * s + s / 2.0))))
}

/// The region's mean brightness less the rest's.
pub fn contrast(px: &[i32], region: &[usize]) -> i32 {
    if region.len() == px.len() {
        return 0;
    }
    let mut inside = vec![false; px.len()];
    let mut sum_in = 0i64;
    for &i in region {
        inside[i] = true;
        sum_in += px[i] as i64;
    }
    let sum_out: i64 = px.iter().zip(&inside).filter(|(_, &i)| !i).map(|(&v, _)| v as i64).sum();
    (sum_in / region.len() as i64 - sum_out / (px.len() - region.len()) as i64) as i32
}

/// A 3×3 box blur, edges clamped.
pub fn blur(src: &[i32], w: usize, h: usize) -> Vec<i32> {
    let mut out = vec![0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0;
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    sum += src[yy * w + xx];
                }
            }
            out[y * w + x] = sum / 9;
        }
    }
    out
}

/// Otsu's threshold: the level that best splits the pixels in two.
pub fn otsu(px: &[i32]) -> i32 {
    let mut hist = [0u32; 256];
    for &v in px {
        hist[v.clamp(0, 255) as usize] += 1;
    }
    let total = px.len() as f64;
    let sum: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();
    let (mut sum_b, mut w_b, mut best, mut cut) = (0.0, 0.0, 0.0, 128);
    for (t, &n) in hist.iter().enumerate() {
        w_b += n as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += t as f64 * n as f64;
        let (m_b, m_f) = (sum_b / w_b, (sum - sum_b) / w_f);
        let between = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if between > best {
            best = between;
            cut = t as i32;
        }
    }
    cut
}

/// The largest 4-connected region brighter than `cut`, as its pixels' indexes.
pub fn largest(px: &[i32], w: usize, h: usize, cut: i32) -> Option<Vec<usize>> {
    let mut seen = vec![false; w * h];
    let mut queue = vec![0usize; w * h];
    let mut best: Option<Vec<usize>> = None;
    for start in 0..px.len() {
        if seen[start] || px[start] <= cut {
            continue;
        }
        let (mut head, mut tail) = (0, 1);
        queue[0] = start;
        seen[start] = true;
        while head < tail {
            let i = queue[head];
            head += 1;
            let (x, y) = (i % w, i / w);
            let near = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in near.into_iter().flatten() {
                if !seen[j] && px[j] > cut {
                    seen[j] = true;
                    queue[tail] = j;
                    tail += 1;
                }
            }
        }
        if tail > best.as_ref().map_or(0, Vec::len) {
            best = Some(queue[..tail].to_vec());
        }
    }
    best
}

/// A region's convex outline: each row's ends, hulled.
pub fn outline(region: &[usize], w: usize) -> Vec<Pt> {
    let mut ends = std::collections::BTreeMap::<usize, (usize, usize)>::new();
    for &i in region {
        let (x, y) = (i % w, i / w);
        let e = ends.entry(y).or_insert((x, x));
        e.0 = e.0.min(x);
        e.1 = e.1.max(x);
    }
    let mut pts = Vec::with_capacity(ends.len() * 4);
    for (y, (l, r)) in ends {
        // A pixel is a square: its outer corners, so a page's sides are
        // where its pixels end.
        let (y, l, r) = (y as f32, l as f32, r as f32);
        pts.extend([
            Pt::new(l - 0.5, y - 0.5),
            Pt::new(l - 0.5, y + 0.5),
            Pt::new(r + 0.5, y - 0.5),
            Pt::new(r + 0.5, y + 0.5),
        ]);
    }
    hull(pts)
}

/// The convex hull (Andrew's monotone chain), clockwise in image coordinates.
pub fn hull(mut ps: Vec<Pt>) -> Vec<Pt> {
    ps.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    ps.dedup();
    if ps.len() < 3 {
        return ps;
    }
    let cross = |o: Pt, a: Pt, b: Pt| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let chain = |ps: &mut dyn Iterator<Item = Pt>| {
        let mut out: Vec<Pt> = vec![];
        for p in ps {
            while out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
        out
    };
    let mut lower = chain(&mut ps.iter().copied());
    lower.extend(chain(&mut ps.iter().rev().copied()));
    lower
}

/// The four outline points that make the largest quad: first the extremes
/// of x+y and x−y, then each moved to whichever outline point between its
/// neighbours covers most, until none moves.
pub fn corners(outline: &[Pt]) -> Option<Quad> {
    let n = outline.len();
    if n < 4 {
        return None;
    }
    let by = |f: &dyn Fn(Pt) -> f32, most: bool| {
        let it = (0..n).map(|i| (i, f(outline[i])));
        match most {
            // The first of equals, as Kotlin's minBy and maxBy take it.
            true => it.fold((0, f32::MIN), |b, c| if c.1 > b.1 { c } else { b }).0,
            false => it.fold((0, f32::MAX), |b, c| if c.1 < b.1 { c } else { b }).0,
        }
    };
    let mut idx = [
        by(&|p| p.x + p.y, false),
        by(&|p| p.x - p.y, true),
        by(&|p| p.x + p.y, true),
        by(&|p| p.x - p.y, false),
    ];
    if idx.iter().collect::<std::collections::BTreeSet<_>>().len() < 4 {
        return None;
    }
    let area = |ix: &[usize; 4]| Quad::of(ix.map(|i| outline[i])).area();
    let mut moved = true;
    let mut rounds = 0;
    while moved && rounds < 16 {
        rounds += 1;
        moved = false;
        for k in 0..4 {
            let (before, after) = (idx[(k + 3) % 4], idx[(k + 1) % 4]);
            let (mut best_i, mut best_a) = (idx[k], area(&idx));
            let mut i = (before + 1) % n;
            while i != after {
                let mut trial = idx;
                trial[k] = i;
                let a = area(&trial);
                if a > best_a + 1e-3 {
                    best_a = a;
                    best_i = i;
                }
                i = (i + 1) % n;
            }
            if best_i != idx[k] {
                idx[k] = best_i;
                moved = true;
            }
        }
    }
    Some(Quad::ordered(idx.map(|i| outline[i])))
}

/// How sharp a frame is: the variance of its Laplacian over its middle,
/// every `step`th pixel. Text in focus is in the hundreds; a blurred frame
/// in the tens.
pub fn sharpness(frame: Luma, step: usize) -> f64 {
    let (x0, y0) = (frame.w / 8, frame.h / 8);
    let (mut n, mut sum, mut sq) = (0u64, 0f64, 0f64);
    let mut y = y0.max(1);
    while y < (frame.h - 1).min(frame.h - y0) {
        let mut x = x0.max(1);
        while x < (frame.w - 1).min(frame.w - x0) {
            let l = frame.at(x - 1, y) + frame.at(x + 1, y) + frame.at(x, y - 1) + frame.at(x, y + 1) - 4 * frame.at(x, y);
            sum += l as f64;
            sq += l as f64 * l as f64;
            n += 1;
            x += step;
        }
        y += step;
    }
    if n == 0 {
        return 0.0;
    }
    let mean = sum / n as f64;
    sq / n as f64 - mean * mean
}

/// Whether a page has held still: each quad told, with the time, and steady
/// once every quad of the last `hold_ms` is within `tolerance` of the
/// frame's diagonal of the newest. A frame with no page starts it again.
#[derive(Clone, Debug)]
pub struct Steady {
    hold_ms: i64,
    tolerance: f32,
    seen: std::collections::VecDeque<(i64, Quad)>,
}

impl Default for Steady {
    fn default() -> Steady {
        Steady::new(1000, 0.02)
    }
}

impl Steady {
    pub fn new(hold_ms: i64, tolerance: f32) -> Steady {
        Steady {
            hold_ms,
            tolerance,
            seen: Default::default(),
        }
    }

    /// The quad of a frame at `at_ms`, `diagonal` the frame's; whether the
    /// page is steady now.
    pub fn tell(&mut self, q: Option<Quad>, at_ms: i64, diagonal: f32) -> bool {
        let Some(q) = q else {
            self.seen.clear();
            return false;
        };
        self.seen.push_back((at_ms, q));
        while self.seen.len() > 1 && at_ms - self.seen[1].0 >= self.hold_ms {
            self.seen.pop_front();
        }
        let limit = self.tolerance * diagonal;
        if self.seen.iter().any(|(_, s)| s.moved(&q) > limit) {
            // Moved: only what is near the newest counts from here.
            while self.seen.front().is_some_and(|(_, s)| s.moved(&q) > limit) {
                self.seen.pop_front();
            }
            return false;
        }
        self.seen.front().is_some_and(|(t, _)| at_ms - t >= self.hold_ms)
    }

    pub fn reset(&mut self) {
        self.seen.clear()
    }
}
