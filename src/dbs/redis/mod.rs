#[cfg(not(test))]
pub mod client;
#[cfg(not(test))]
pub use client::{new_pool, redis_delete, redis_get, redis_set, redis_set_ex};

#[cfg(test)]
mod test_utils;
#[cfg(test)]
pub use test_utils::*;
