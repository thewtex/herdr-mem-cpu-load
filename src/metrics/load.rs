//! Load average metrics.

pub use crate::sys::LoadAverages;

/// Load as a 0-100 percentage, matching the original's colour-lookup index:
/// `averages[0] / cpu_count * 0.5 * 100`, capped at 100.
///
/// The 0.5 factor means a load equal to twice the CPU count reads as 100%.
#[must_use]
pub fn load_percent(load: &LoadAverages, cpu_count: u32) -> u32 {
    if cpu_count == 0 {
        return 0;
    }
    let percent = load.one / f64::from(cpu_count) * 0.5 * 100.0;
    if percent <= 0.0 {
        0
    } else if percent >= 100.0 {
        100
    } else {
        percent as u32
    }
}

/// One minute load average normalised by the CPU count, which later phases use
/// for threshold colouring.
#[must_use]
pub fn load_per_core(load: &LoadAverages, cpu_count: u32) -> f64 {
    if cpu_count == 0 {
        0.0
    } else {
        load.one / f64::from(cpu_count)
    }
}

#[cfg(test)]
mod tests {
    use super::{load_per_core, load_percent, LoadAverages};

    fn averages(one: f64) -> LoadAverages {
        LoadAverages {
            one,
            five: 0.0,
            fifteen: 0.0,
        }
    }

    #[test]
    fn load_percent_halves_the_per_core_load() {
        assert_eq!(load_percent(&averages(0.0), 8), 0);
        assert_eq!(load_percent(&averages(8.0), 8), 50);
        assert_eq!(load_percent(&averages(16.0), 8), 100);
    }

    #[test]
    fn load_percent_is_capped_at_one_hundred() {
        assert_eq!(load_percent(&averages(64.0), 8), 100);
    }

    #[test]
    fn zero_cpu_count_is_not_a_division_by_zero() {
        assert_eq!(load_percent(&averages(4.0), 0), 0);
        assert!(load_per_core(&averages(4.0), 0).abs() < f64::EPSILON);
    }

    #[test]
    fn load_per_core_divides_by_the_cpu_count() {
        assert!((load_per_core(&averages(4.0), 8) - 0.5).abs() < 1e-9);
    }
}
