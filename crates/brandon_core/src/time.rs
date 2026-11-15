/// Frame timing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Time {
    delta: f32,
    elapsed: f64,
    frame: u64,
}

impl Time {
    /// Frames longer than this are treated as this long, so a hitch (or a
    /// breakpoint) does not make everything jump.
    pub const MAX_DELTA: f32 = 0.1;

    /// Seconds since the previous frame.
    pub fn delta(&self) -> f32 {
        self.delta
    }

    /// Seconds since the game started.
    pub fn elapsed(&self) -> f32 {
        self.elapsed as f32
    }

    /// Number of frames since the game started. The first frame is `1`.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Moves time forward by one frame of `delta` seconds.
    pub fn advance(&mut self, delta: f32) {
        self.delta = delta.clamp(0.0, Self::MAX_DELTA);
        self.elapsed += self.delta as f64;
        self.frame += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_clamps_long_frames() {
        let mut t = Time::default();
        t.advance(0.016);
        t.advance(5.0);
        assert_eq!(t.frame(), 2);
        assert_eq!(t.delta(), Time::MAX_DELTA);
        assert!((t.elapsed() - 0.116).abs() < 1e-5);
    }
}
