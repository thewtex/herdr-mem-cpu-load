//! Memory metric types.

pub use crate::sys::MemoryStatus;
use crate::sys::SysError;

/// Memory status string output mode.
///
/// * `Default` renders `2885/7987MB`
/// * `Free` renders `4.98GB`
/// * `UsagePercent` renders `36.12%`
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryMode {
    #[default]
    Default = 0,
    Free = 1,
    UsagePercent = 2,
}

impl TryFrom<u8> for MemoryMode {
    type Error = SysError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Default),
            1 => Ok(Self::Free),
            2 => Ok(Self::UsagePercent),
            other => Err(SysError::new(format!(
                "invalid memory mode `{other}`, expected 0, 1, or 2"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryMode;

    #[test]
    fn memory_mode_round_trips_from_u8() {
        assert_eq!(MemoryMode::try_from(0), Ok(MemoryMode::Default));
        assert_eq!(MemoryMode::try_from(1), Ok(MemoryMode::Free));
        assert_eq!(MemoryMode::try_from(2), Ok(MemoryMode::UsagePercent));
        assert!(MemoryMode::try_from(3).is_err());
        assert_eq!(MemoryMode::default(), MemoryMode::Default);
    }
}
