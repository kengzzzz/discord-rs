pub mod client;

#[cfg(feature = "mock-redis")]
pub(crate) mod tests;

pub use client::new_pool;

#[cfg(not(feature = "mock-redis"))]
pub use client::{redis_delete, redis_get, redis_set, redis_set_ex};

#[cfg(feature = "mock-redis")]
pub use tests::{redis_delete, redis_get, redis_set, redis_set_ex};
