#[macro_export]
macro_rules! send_with_fallback {
    ($ctx:expr, $user:expr, $fallback:expr, |$builder:ident| $body:block) => {{
        let __ctx = $ctx.clone();
        let __user = $user;
        let __fallback = $fallback;
        (async move {
            if let Ok(dm_channel) = $crate::open_dm!(__ctx.http, __user).await {
                let $builder = __ctx.http.create_message(dm_channel.id);
                if async { $body }.await.is_ok() {
                    return;
                }
            }
            let $builder = __ctx.http.create_message(__fallback);
            if let Err(e) = async { $body }.await {
                tracing::error!(error = %e, "Failed to send fallback message");
            }
        }).await;
    }};
}
