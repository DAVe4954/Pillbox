//! Turning mouse-wheel travel into whole steps, used to switch tabs with the wheel over the
//! tab row. A notch of a normal wheel is 1.0; precision touchpads send many smaller steps.

/// Wheel travel gathered until it adds up to whole steps.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct WheelSteps {
    left: f32,
}

impl WheelSteps {
    /// Add `delta` (positive = away from the user) and return the whole steps it completes,
    /// positive for away. Turning the other way first drops what was gathered.
    pub fn add(&mut self, delta: f32) -> i32 {
        if !delta.is_finite() {
            return 0;
        }
        if self.left * delta < 0.0 {
            self.left = 0.0;
        }
        self.left += delta;
        let steps = self.left.trunc();
        self.left -= steps;
        steps as i32
    }

    pub fn reset(&mut self) {
        self.left = 0.0;
    }
}

/// The index `by` places after `at` in a list of `len`, held at the ends (no wrapping).
pub fn step_clamped(at: usize, len: usize, by: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let to = at as i64 + i64::from(by);
    to.clamp(0, len as i64 - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_notches_step_at_once() {
        let mut w = WheelSteps::default();
        assert_eq!(w.add(1.0), 1);
        assert_eq!(w.add(-2.0), -2);
    }

    #[test]
    fn touchpad_steps_add_up() {
        let mut w = WheelSteps::default();
        assert_eq!(w.add(0.4), 0);
        assert_eq!(w.add(0.4), 0);
        assert_eq!(w.add(0.4), 1);
        assert_eq!(w.add(0.5), 0, "0.2 was left, 0.7 now");
    }

    #[test]
    fn turning_back_drops_the_rest() {
        let mut w = WheelSteps::default();
        assert_eq!(w.add(0.9), 0);
        assert_eq!(w.add(-0.5), 0, "the 0.9 is gone, not netted to 0.4");
        assert_eq!(w.add(-0.5), -1);
        w.reset();
        assert_eq!(w.add(0.5), 0);
        assert_eq!(w.add(f32::NAN), 0);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        assert_eq!(step_clamped(0, 5, -1), 0);
        assert_eq!(step_clamped(4, 5, 3), 4);
        assert_eq!(step_clamped(2, 5, 1), 3);
        assert_eq!(step_clamped(2, 5, -9), 0);
        assert_eq!(step_clamped(0, 0, 1), 0);
    }
}
