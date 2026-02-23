//! # ShellType - Trait Implementations
//!
//! This module contains trait implementations for `ShellType`.
//!
//! ## Implemented Traits
//!
//! - `FromStr`
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use super::types::ShellType;

impl std::str::FromStr for ShellType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bash" => Ok(ShellType::Bash),
            "zsh" => Ok(ShellType::Zsh),
            "fish" => Ok(ShellType::Fish),
            "powershell" | "pwsh" => Ok(ShellType::PowerShell),
            _ => {
                Err(
                    format!(
                        "Unknown shell type: {}. Supported: bash, zsh, fish, powershell",
                        s
                    ),
                )
            }
        }
    }
}

