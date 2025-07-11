#[macro_export]
macro_rules! handle_ephemeral {
    ($http:expr, $interaction:ident, $name:literal, $body:block) => {{
        use tracing::Instrument;
        async move {
            if let Err(e) = $crate::guild_command!($http, $interaction, true, {
                $crate::defer_interaction!($http, &$interaction, true).await?;
                $body
                Ok::<_, anyhow::Error>(())
            })
            .await
            {
                tracing::error!(error = %e, "error handling {}", $name);
            }
        }
        .instrument(tracing::info_span!("command", name = $name))
        .await;
    }};
}
