use super::models::ChatEntry;
use super::*;
use once_cell::sync::OnceCell as SyncOnceCell;

pub(super) static GENERATE_OVERRIDE: SyncOnceCell<
    Box<dyn Fn(Vec<Content>) -> google_ai_rs::genai::Response + Send + Sync>,
> = SyncOnceCell::new();
#[allow(clippy::type_complexity)]
pub(super) static SUMMARIZE_OVERRIDE: SyncOnceCell<
    Box<dyn Fn(&[ChatEntry]) -> String + Send + Sync>,
> = SyncOnceCell::new();

pub(crate) fn set_generate_override<F>(f: F)
where
    F: Fn(Vec<Content>) -> google_ai_rs::genai::Response + Send + Sync + 'static,
{
    let _ = GENERATE_OVERRIDE.set(Box::new(f));
}

pub(crate) fn set_summarize_override<F>(f: F)
where
    F: Fn(&[ChatEntry]) -> String + Send + Sync + 'static,
{
    let _ = SUMMARIZE_OVERRIDE.set(Box::new(f));
}
