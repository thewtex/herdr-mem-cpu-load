//! A load average for platforms whose kernel does not keep one.
//!
//! Windows has no load average at all — the original `tmux-mem-cpu-load`
//! Windows port simply printed nothing where the three numbers go. Rather than
//! leave the row empty, the daemon runs the same exponentially weighted moving
//! average the Linux kernel uses, fed with the number of busy cores it just
//! measured.
//!
//! The module is compiled on every platform so the maths is unit tested on the
//! machine that develops it, not only on the one that ships it.

use std::time::Duration;

use crate::sys::LoadAverages;

/// The three averaging windows, in seconds: one, five, and fifteen minutes.
pub const DECAY_WINDOWS_SECS: [f64; 3] = [60.0, 300.0, 900.0];

/// An exponentially weighted moving average of the busy core count.
///
/// The Linux kernel folds a new sample in with `load = load * e^(-dt/T) +
/// sample * (1 - e^(-dt/T))` for each window `T`. Computing the decay factor
/// from the real elapsed time rather than a fixed tick keeps the result correct
/// when the sampling interval changes or a tick runs late.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadEmulator {
    one: f64,
    five: f64,
    fifteen: f64,
    seeded: bool,
}

impl LoadEmulator {
    /// An emulator with no history yet. The first [`update`](Self::update)
    /// seeds all three windows rather than decaying up from zero, so the very
    /// first reading is the current load instead of a meaningless 0.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            one: 0.0,
            five: 0.0,
            fifteen: 0.0,
            seeded: false,
        }
    }

    /// Fold `busy_cores` — `cpu_percent / 100 * cpu_count` — into the three
    /// windows and return the new averages.
    pub fn update(&mut self, busy_cores: f64, dt: Duration) -> LoadAverages {
        // A NaN or negative reading would poison every future value, since the
        // state is fed back into itself on the next call.
        let sample = if busy_cores.is_finite() {
            busy_cores.max(0.0)
        } else {
            0.0
        };

        if !self.seeded {
            self.one = sample;
            self.five = sample;
            self.fifteen = sample;
            self.seeded = true;
            return self.current();
        }

        let seconds = dt.as_secs_f64();
        let values = [&mut self.one, &mut self.five, &mut self.fifteen];
        for (value, window) in values.into_iter().zip(DECAY_WINDOWS_SECS) {
            let factor = (-seconds / window).exp();
            *value = value.mul_add(factor, sample * (1.0 - factor));
        }
        self.current()
    }

    /// The averages as they stand, without folding in a new sample.
    #[must_use]
    pub const fn current(&self) -> LoadAverages {
        LoadAverages {
            one: self.one,
            five: self.five,
            fifteen: self.fifteen,
        }
    }

    /// Whether a sample has been folded in yet.
    #[must_use]
    pub const fn is_seeded(&self) -> bool {
        self.seeded
    }
}

#[cfg(test)]
mod tests {
    use super::LoadEmulator;
    use std::time::Duration;

    const MINUTE: Duration = Duration::from_secs(60);
    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn the_first_sample_seeds_all_three_windows() {
        let mut emulator = LoadEmulator::new();
        assert!(!emulator.is_seeded());

        let load = emulator.update(3.5, MINUTE);
        assert!(emulator.is_seeded());
        assert!((load.one - 3.5).abs() < f64::EPSILON);
        assert!((load.five - 3.5).abs() < f64::EPSILON);
        assert!((load.fifteen - 3.5).abs() < f64::EPSILON);
        assert_eq!(load, emulator.current());
    }

    #[test]
    fn a_constant_load_stays_put() {
        let mut emulator = LoadEmulator::new();
        emulator.update(2.0, SECOND);
        for _ in 0..1000 {
            emulator.update(2.0, Duration::from_secs(2));
        }

        let load = emulator.current();
        assert!((load.one - 2.0).abs() < 1e-9, "one minute: {}", load.one);
        assert!((load.five - 2.0).abs() < 1e-9, "five minute: {}", load.five);
        assert!(
            (load.fifteen - 2.0).abs() < 1e-9,
            "fifteen minute: {}",
            load.fifteen
        );
    }

    #[test]
    fn one_window_of_a_step_change_reaches_one_minus_one_over_e() {
        let mut emulator = LoadEmulator::new();
        emulator.update(0.0, SECOND);
        // Sixty one-second steps must compose into exactly one 60 second decay.
        for _ in 0..60 {
            emulator.update(4.0, SECOND);
        }

        let load = emulator.current();
        // 4 * (1 - e^-1) ~= 2.5285
        assert!(
            (load.one - 4.0 * (1.0 - (-1.0_f64).exp())).abs() < 0.05,
            "one minute average was {}",
            load.one
        );
        assert!(
            load.fifteen < load.five && load.five < load.one,
            "longer windows must lag: {load:?}"
        );
    }

    #[test]
    fn values_stay_between_zero_and_the_largest_sample() {
        let mut emulator = LoadEmulator::new();
        let samples = [0.0, 8.0, 0.0, 3.0, 0.0, 8.0, 1.5, 0.0];
        let max = 8.0;

        emulator.update(0.0, SECOND);
        for _ in 0..50 {
            for sample in samples {
                let load = emulator.update(sample, Duration::from_secs(5));
                for value in [load.one, load.five, load.fifteen] {
                    assert!(
                        (0.0..=max).contains(&value),
                        "{value} escaped 0..={max} for sample {sample}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_nonsense_sample_cannot_poison_the_state() {
        let mut emulator = LoadEmulator::new();
        emulator.update(f64::NAN, SECOND);
        emulator.update(-5.0, SECOND);
        emulator.update(f64::INFINITY, SECOND);

        let load = emulator.current();
        assert!(load.one.is_finite() && load.one >= 0.0, "{load:?}");
        assert!(load.five.is_finite() && load.five >= 0.0, "{load:?}");
        assert!(load.fifteen.is_finite() && load.fifteen >= 0.0, "{load:?}");
    }
}
