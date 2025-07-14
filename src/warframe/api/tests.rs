use once_cell::sync::OnceCell;

pub(super) static BASE_URL_OVERRIDE: OnceCell<String> = OnceCell::new();

pub(crate) fn set_base_url(url: &str) {
    let _ = BASE_URL_OVERRIDE.set(url.to_string());
}
