//! Damped spring integrated with a fixed timestep.

/// Simulation step. Rendering may run at 30, 60 or 90 fps; the physics always advances in
/// 1/120 s steps so the motion is identical at every frame rate.
pub const STEP: f32 = 1.0 / 120.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringParams {
    /// Slightly under-damped: about 2.5% overshoot, settles in roughly a third of a second.
    pub const DEFAULT: Self = Self {
        stiffness: 340.0,
        damping: 28.0,
    };

    /// Same motion curve played `speed` times faster (1.0 = unchanged).
    pub fn with_speed(self, speed: f32) -> Self {
        let s = speed.clamp(0.1, 10.0);
        Self {
            stiffness: self.stiffness * s * s,
            damping: self.damping * s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
        }
    }

    /// Advance by `dt` seconds (semi-implicit Euler, stable for the stiffness range we use).
    pub fn step(&mut self, p: SpringParams, dt: f32) {
        let accel = -p.stiffness * (self.value - self.target) - p.damping * self.velocity;
        self.velocity += accel * dt;
        self.value += self.velocity * dt;
    }

    pub fn is_settled(&self) -> bool {
        (self.value - self.target).abs() < 0.05 && self.velocity.abs() < 0.05
    }

    /// Jump to the target and stop.
    pub fn snap(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

/// Converts variable frame times into a whole number of fixed steps.
#[derive(Debug, Default, Clone, Copy)]
pub struct FixedStep {
    acc: f32,
}

impl FixedStep {
    /// Longest frame gap we simulate; a stall longer than this slows time instead of exploding.
    const MAX_ELAPSED: f32 = 0.1;

    pub fn reset(&mut self) {
        self.acc = 0.0;
    }

    /// Returns how many `STEP`s to run for `elapsed` seconds of wall time.
    pub fn advance(&mut self, elapsed: f32) -> u32 {
        if !elapsed.is_finite() || elapsed <= 0.0 {
            return 0;
        }
        self.acc += elapsed.min(Self::MAX_ELAPSED);
        let mut steps = 0;
        while self.acc >= STEP {
            self.acc -= STEP;
            steps += 1;
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(spring: &mut Spring, p: SpringParams, seconds: f32) -> f32 {
        let mut peak = spring.value;
        let steps = (seconds / STEP) as u32;
        for _ in 0..steps {
            spring.step(p, STEP);
            peak = peak.max(spring.value);
        }
        peak
    }

    #[test]
    fn converges_to_target() {
        let mut s = Spring::new(200.0);
        s.target = 400.0;
        run(&mut s, SpringParams::DEFAULT, 1.0);
        assert!(s.is_settled(), "not settled: {s:?}");
        assert!((s.value - 400.0).abs() < 0.05);
    }

    #[test]
    fn overshoot_is_small() {
        let mut s = Spring::new(0.0);
        s.target = 100.0;
        let peak = run(&mut s, SpringParams::DEFAULT, 2.0);
        assert!(peak > 100.0, "expected a little overshoot, peak {peak}");
        assert!(peak < 105.0, "overshoot too large, peak {peak}");
    }

    #[test]
    fn stays_finite_when_retargeted_mid_flight() {
        let mut s = Spring::new(0.0);
        for i in 0..2000 {
            s.target = if (i / 7) % 2 == 0 { 400.0 } else { 0.0 };
            s.step(SpringParams::DEFAULT, STEP);
            assert!(s.value.is_finite() && s.velocity.is_finite());
            assert!((-200.0..=600.0).contains(&s.value));
        }
    }

    #[test]
    fn speed_two_settles_about_twice_as_fast() {
        let time_to_settle = |p: SpringParams| {
            let mut s = Spring::new(0.0);
            s.target = 100.0;
            let mut t = 0.0;
            while !s.is_settled() && t < 5.0 {
                s.step(p, STEP);
                t += STEP;
            }
            t
        };
        let slow = time_to_settle(SpringParams::DEFAULT);
        let fast = time_to_settle(SpringParams::DEFAULT.with_speed(2.0));
        let ratio = slow / fast;
        assert!((1.6..=2.4).contains(&ratio), "ratio {ratio}");
    }

    #[test]
    fn snap_stops_the_spring() {
        let mut s = Spring::new(0.0);
        s.target = 10.0;
        s.step(SpringParams::DEFAULT, STEP);
        s.snap();
        assert_eq!(s.value, 10.0);
        assert_eq!(s.velocity, 0.0);
        assert!(s.is_settled());
    }

    #[test]
    fn fixed_step_is_frame_rate_independent() {
        // One second of wall time must give 120 steps whether it arrives as 30, 60 or 90 frames.
        for fps in [30u32, 60, 90] {
            let mut f = FixedStep::default();
            let total: u32 = (0..fps).map(|_| f.advance(1.0 / fps as f32)).sum();
            assert!((119..=121).contains(&total), "{fps} fps gave {total} steps");
        }
    }

    #[test]
    fn fixed_step_clamps_stalls_and_rejects_bad_input() {
        let mut f = FixedStep::default();
        let steps = f.advance(5.0); // clamped to 0.1 s, i.e. about 12 steps rather than 600
        assert!((11..=12).contains(&steps), "{steps}");
        assert_eq!(f.advance(-1.0), 0);
        assert_eq!(f.advance(f32::NAN), 0);
    }
}
