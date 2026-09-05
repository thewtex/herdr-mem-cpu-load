//! Memory metric types.

use serde::{Deserialize, Serialize};

pub use crate::sys::MemoryStatus;
use crate::sys::SysError;

/// Memory status string output mode.
///
/// * `Default` renders `2885/7987MB`
/// * `Free` renders `4.98GB`
/// * `UsagePercent` renders `36.12%`
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(try_from = "u8", into = "u8")]
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

impl From<MemoryMode> for u8 {
    fn from(mode: MemoryMode) -> Self {
        mode as Self
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
        assert_eq!(u8::from(MemoryMode::UsagePercent), 2);
    }

    #[test]
    fn memory_mode_is_a_number_in_a_config_file() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct Wrapper {
            mem_mode: MemoryMode,
        }

        let parsed: Wrapper = toml::from_str("mem_mode = 1").expect("parses");
        assert_eq!(parsed.mem_mode, MemoryMode::Free);
        assert_eq!(
            toml::to_string(&parsed).expect("serialises").trim(),
            "mem_mode = 1"
        );
        assert!(toml::from_str::<Wrapper>("mem_mode = 9").is_err());
    }
}
