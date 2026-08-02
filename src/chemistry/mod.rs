//! Chemistry of the universe (reserved).
//!
//! **The explicit goal of the project is that this module never needs to
//! exist as specific code.** If chemistry emerges, it will be a consequence of
//! the laws in `crate::physics`, not of "create water" or "create bond"
//! functions.
//!
//! This module exists to delimit the space and document the rule.

/// Principles that future laws must respect.
pub mod principles {
    /// A "reaction" must never be programmed as such: it must be the sum of
    /// low-level interactions (bond = local energy < threshold, etc.).
    pub const NO_HARDCODED_CHEMISTRY: &str =
        "implementing chemical species explicitly is prohibited";
}
