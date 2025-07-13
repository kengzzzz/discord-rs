pub mod client;

#[cfg(test)]
pub(crate) mod tests;

pub use client::{REDIS_POOL, new_pool};

#[cfg(not(test))]
pub use client::{redis_delete, redis_get, redis_set, redis_set_ex};

#[cfg(test)]
pub use tests::{redis_delete, redis_get, redis_set, redis_set_ex};
