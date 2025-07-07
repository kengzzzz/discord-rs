#[macro_export]
macro_rules! defer_interaction {
    ($http:expr, $interaction:expr, $ephemeral:expr) => {{
        use twilight_model::channel::message::MessageFlags;
        use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
        use twilight_util::builder::InteractionResponseDataBuilder;
        async {
            let mut builder = InteractionResponseDataBuilder::new();
            if $ephemeral {
                builder = builder.flags(MessageFlags::EPHEMERAL);
            }
            let data = builder.build();
            $http
                .interaction($interaction.application_id)
                .create_response(
                    $interaction.id,
                    &$interaction.token,
                    &InteractionResponse {
                        kind: InteractionResponseType::DeferredChannelMessageWithSource,
                        data: Some(data),
                    },
                )
                .await?;
            Ok::<_, anyhow::Error>(())
        }
    }};
}
