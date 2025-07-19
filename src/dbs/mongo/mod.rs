#[cfg(not(any(test, feature = "test-utils")))]
pub mod client;
#[cfg(not(any(test, feature = "test-utils")))]
pub use client::MongoDB;

#[cfg(any(test, feature = "test-utils"))]
mod test_utils;
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::MongoDB;

pub mod models;
pub mod monitor;
pub mod watcher;
pub mod watchers;
