#[macro_export]
macro_rules! send_with_fallback {
    ($http:expr, $user:expr, $fallback:expr, |$builder:ident| $body:block) => {
        (async move {
            if let Ok(dm_channel) = $crate::open_dm!($http, $user).await {
                let $builder = $http.create_message(dm_channel.id);
                if let Ok(_) = async { $body }.await {
                    return;
                }
            }
            let $builder = $http.create_message($fallback);
            if let Err(e) = async { $body }.await {
                tracing::error!(error = %e, "Failed to send fallback message");
            }
        })
        .await;
    };
}
