#[cfg(not(test))]
pub mod client;
#[cfg(not(test))]
pub use client::MongoDB;

#[cfg(test)]
mod test_utils;
#[cfg(test)]
pub use test_utils::MongoDB;

pub mod models;
pub mod monitor;
pub mod watcher;
pub mod watchers;
