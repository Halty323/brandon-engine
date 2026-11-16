/// A small, fast random number generator for gameplay.
///
/// Not for anything that needs to be secure.
#[derive(Clone, Debug)]
pub struct Random {
    state: u64,
}

impl Random {
    /// A generator that gives different numbers on every run.
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        Self::from_seed(nanos ^ (std::process::id() as u64).rotate_left(32))
    }

    /// A generator that gives the same numbers for the same seed.
    pub fn from_seed(seed: u64) -> Self {
        let mut random = Random { state: seed };
        random.next_u64(); // Mix the seed so nearby seeds give unrelated numbers.
        random
    }

    /// The next raw 64-bit number (SplitMix64).
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number from `0.0` up to, but not including, `1.0`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// A number from `low` up to, but not including, `high`.
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.unit()
    }

    /// A whole number from `low` to `high`, including both.
    pub fn int(&mut self, low: i32, high: i32) -> i32 {
        if high <= low {
            return low;
        }
        let span = (high as i64 - low as i64 + 1) as u64;
        (low as i64 + (self.next_u64() % span) as i64) as i32
    }

    /// `true` with the given probability, from `0.0` (never) to `1.0` (always).
    pub fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }

    /// A random item of a slice, or `None` if it is empty.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[(self.next_u64() % items.len() as u64) as usize])
        }
    }
}

impl Default for Random {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_numbers() {
        let (mut a, mut b) = (Random::from_seed(7), Random::from_seed(7));
        assert!((0..100).all(|_| a.next_u64() == b.next_u64()));
        assert_ne!(
            Random::from_seed(7).next_u64(),
            Random::from_seed(8).next_u64()
        );
    }

    #[test]
    fn values_stay_in_range_and_spread_out() {
        let mut random = Random::from_seed(1);
        let mut sum = 0.0;
        let mut seen = [false; 6];
        for _ in 0..10_000 {
            let u = random.unit();
            assert!((0.0..1.0).contains(&u));
            sum += u;
            let r = random.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&r));
            let i = random.int(1, 6);
            assert!((1..=6).contains(&i));
            seen[(i - 1) as usize] = true;
        }
        assert!((sum / 10_000.0 - 0.5).abs() < 0.02);
        assert!(seen.iter().all(|s| *s));
        assert_eq!(random.int(5, 5), 5);
        let _ = random.int(i32::MIN, i32::MAX); // the full range must not overflow
        assert!(!random.chance(0.0) && random.chance(1.0));
        assert_eq!(random.pick::<u8>(&[]), None);
        assert_eq!(random.pick(&[9]), Some(&9));
    }
}
