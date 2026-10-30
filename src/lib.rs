//! Portable utility logic. Terminal and filesystem adapters live in the binary.
pub mod calculator;
pub mod calendar;
pub mod model;
pub mod units;
pub type Result<T> = std::result::Result<T, String>;
