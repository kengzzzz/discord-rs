#[cfg(not(any(test, feature = "test-utils")))]
mod context;
#[cfg(not(any(test, feature = "test-utils")))]
pub use context::Context;
#[cfg(not(any(test, feature = "test-utils")))]
mod builder;
#[cfg(not(any(test, feature = "test-utils")))]
pub use builder::ContextBuilder;

#[cfg(any(test, feature = "test-utils"))]
mod test_utils;
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::mock_builder::{self, ContextBuilder};
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::mock_context::Context;
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::mock_http;
#[cfg(any(test, feature = "test-utils"))]
pub use test_utils::mock_reqwest;
