#[cfg(not(any(test, feature = "test-utils")))]
pub mod client;
#[cfg(not(any(test, feature = "test-utils")))]
pub use client::{new_pool, redis_delete, redis_get, redis_set, redis_set_ex};

#[cfg(any(test, feature = "test-utils"))]
mod test_utils;
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::*;
