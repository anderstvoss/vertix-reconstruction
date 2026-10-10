//! `Math.random()` and KRP's helpers on top of it.

#![forbid(unsafe_code)]

use std::cell::Cell;

thread_local! {
    static STATE: Cell<u64> = Cell::new(seed());
}

fn seed() -> u64 {
    let t = (crate::platform::now_ms() * 1000.0) as u64;
    t ^ 0x9E37_79B9_7F4A_7C15
}

/// A float in `[0, 1)` (xorshift64*).
#[must_use]
pub fn random() -> f64 {
    STATE.with(|s| {
        let mut x = s.get().max(1);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// KRP `randomFloat`.
#[must_use]
pub fn random_float(min: f64, max: f64) -> f64 {
    min + random() * (max - min)
}

/// KRP `randomInt`: inclusive.
#[must_use]
pub fn random_int(min: i64, max: i64) -> i64 {
    (random() * (max - min + 1) as f64).floor() as i64 + min
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_hold() {
        for _ in 0..1000 {
            let f = random_float(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&f));
            let i = random_int(5, 7);
            assert!((5..=7).contains(&i));
        }
    }
}
