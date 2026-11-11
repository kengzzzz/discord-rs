use twilight_http::{api_error::ApiError, error::ErrorType};

pub const UNKNOWN_CHANNEL: u64 = 10003;
pub const UNKNOWN_MEMBER: u64 = 10007;
pub const UNKNOWN_MESSAGE: u64 = 10008;
pub const UNKNOWN_ROLE: u64 = 10011;
pub const UNKNOWN_INTERACTION: u64 = 10062;
pub const UNKNOWN_WEBHOOK: u64 = 10015;
pub const INVALID_WEBHOOK_TOKEN: u64 = 50027;
pub const CANNOT_DM: u64 = 50007;
pub const MISSING_PERMISSIONS: u64 = 50013;
pub const MISSING_ACCESS: u64 = 50001;

pub fn code(error: &anyhow::Error) -> Option<u64> {
    if let Some(webhook) = error.downcast_ref::<WebhookError>() {
        return webhook.code;
    }
    let http = error.downcast_ref::<twilight_http::Error>()?;
    http_code(http)
}

#[derive(Debug, thiserror::Error)]
#[error("Discord answered {status} (code {code:?}): {detail}")]
pub struct WebhookError {
    pub status: u16,
    pub code: Option<u64>,
    pub detail: String,
}

pub fn expired(error: &anyhow::Error) -> bool {
    matches!(
        code(error),
        Some(UNKNOWN_INTERACTION | UNKNOWN_WEBHOOK | INVALID_WEBHOOK_TOKEN)
    )
}

pub fn http_code(error: &twilight_http::Error) -> Option<u64> {
    match error.kind() {
        ErrorType::Response { error: ApiError::General(general), .. } => Some(general.code),
        _ => None,
    }
}

pub fn status(error: &twilight_http::Error) -> Option<u16> {
    match error.kind() {
        ErrorType::Response { status, .. } => Some(status.get()),
        _ => None,
    }
}

/// Only a 4xx proves Discord didn't apply the request; a 5xx can come after it did.
pub fn refused(error: &twilight_http::Error) -> bool {
    status(error).is_some_and(refused_status)
}

fn refused_status(status: u16) -> bool {
    (400..500).contains(&status)
}

pub fn is(error: &twilight_http::Error, wanted: u64) -> bool {
    http_code(error) == Some(wanted)
}

pub fn is_forbidden(error: &twilight_http::Error) -> bool {
    status(error) == Some(403)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_expired_webhook_token_counts_as_expired() {
        let error =
            |code| anyhow::Error::from(WebhookError { status: 401, code, detail: String::new() });
        assert!(expired(&error(Some(INVALID_WEBHOOK_TOKEN))));
        assert!(expired(&error(Some(UNKNOWN_WEBHOOK))));
        assert!(expired(&error(Some(UNKNOWN_INTERACTION))));
        assert!(!expired(&error(Some(MISSING_PERMISSIONS))));
        assert!(!expired(&error(None)));
    }

    #[test]
    fn only_client_errors_count_as_refused() {
        assert!(refused_status(400));
        assert!(refused_status(403));
        assert!(refused_status(429));
        assert!(!refused_status(500));
        assert!(!refused_status(503));
        assert!(!refused_status(200));
    }
}
