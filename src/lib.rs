//! Portable utility logic. Terminal and filesystem adapters live in the binary.
pub mod calculator;
pub mod calendar;
pub mod model;
pub mod sample;
pub mod units;
pub mod weather;
pub mod workbench;
pub type Result<T> = std::result::Result<T, String>;
