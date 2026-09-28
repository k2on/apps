//! The only non-determinism a mutation gets: a fresh id and the clock,
//! drawn once at the originating peer and frozen in the entry forever.
//! `petros::AutoCtx`, for ArkDB.

use ark::eval::Args;
use ark::ir::{Auto, Function};
use ark::value::{Id, Value};

/// Where fresh ids and the time come from.
#[derive(Clone, Debug)]
pub enum Autos {
    /// The OS's randomness and the wall clock (`Date.now()` in a browser).
    System,
    /// A seeded generator and a clock that ticks one millisecond per draw
    /// from `start_ms`: reproducible, for tests and simulations.
    Seeded { state: u64, now_ms: i64 },
}

impl Autos {
    pub fn system() -> Autos {
        Autos::System
    }

    pub fn seeded(seed: u64) -> Autos {
        Autos::Seeded {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
            now_ms: 1_700_000_000_000,
        }
    }

    /// A fresh id, laid out as a version-4 UUID.
    pub fn new_id(&mut self) -> Id {
        let mut b = [0u8; 16];
        match self {
            Autos::System => {
                // A peer with no randomness cannot author anything honestly.
                getrandom::getrandom(&mut b).expect("the platform gives no randomness");
            }
            Autos::Seeded { state, .. } => {
                for chunk in b.chunks_mut(8) {
                    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                    let x = *state ^ (*state >> 33);
                    chunk.copy_from_slice(&x.to_le_bytes()[..chunk.len()]);
                }
            }
        }
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        b
    }

    /// Milliseconds since the Unix epoch.
    pub fn now_ms(&mut self) -> i64 {
        match self {
            Autos::System => now_ms(),
            Autos::Seeded { now_ms, .. } => {
                *now_ms += 1;
                *now_ms
            }
        }
    }

    /// Every auto a function declares, drawn now — including one only an
    /// untaken branch reads, because replay reads the frozen value.
    pub fn draw(&mut self, f: &Function) -> Args {
        f.autos
            .iter()
            .map(|(name, auto)| {
                let v = match auto {
                    Auto::NewId(_) => Value::Id(self.new_id()),
                    Auto::Now => Value::Int(self.now_ms()),
                };
                (name.clone(), v)
            })
            .collect()
    }
}

/// The wall clock, in milliseconds since the epoch. `SystemTime::now()`
/// panics on wasm32-unknown-unknown, so a browser asks `Date`.
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// A monotonic clock for timers (backoff, keepalive), in milliseconds from
/// an arbitrary start. `Instant` panics on wasm32-unknown-unknown too.
pub fn monotonic_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        static START: OnceLock<std::time::Instant> = OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }
}
