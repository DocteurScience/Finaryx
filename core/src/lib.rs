//! Finaryx core: data model, import, statistics and forecast.
//!
//! This crate must not depend on GTK so that it stays fully unit-tested.

pub mod accounts;
pub mod categories;
pub mod categorize;
pub mod dates;
pub mod db;
pub mod demo;
pub mod export;
pub mod forecast;
pub mod i18n;
pub mod importers;
pub mod models;
pub mod money;
pub mod normalize;
pub mod repo;
pub mod scale;
pub mod stats;
pub mod transfers;

/// Re-exported so that the interface uses the same SQLite types.
pub use rusqlite;
