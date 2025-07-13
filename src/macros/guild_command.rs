#[macro_export]
macro_rules! guild_command {
    ($http:expr, $interaction:ident, $ephemeral:expr, $body:block) => {{
        async {
            use twilight_model::channel::message::MessageFlags;
            use twilight_model::http::interaction::{
                InteractionResponse, InteractionResponseData, InteractionResponseType,
            };
            use $crate::utils::embed;

            if $interaction.guild_id.is_none() {
                if let Ok(embed) = embed::guild_only_embed() {
                    if let Err(e) = $http
                        .interaction($interaction.application_id)
                        .create_response(
                            $interaction.id,
                            &$interaction.token,
                            &InteractionResponse {
                                kind: InteractionResponseType::ChannelMessageWithSource,
                                data: Some(InteractionResponseData {
                                    embeds: Some(vec![embed]),
                                    flags: Some(MessageFlags::EPHEMERAL),
                                    ..Default::default()
                                }),
                            },
                        )
                        .await
                    {
                        tracing::warn!(error = %e, "failed to send guild-only response");
                    }
                }
                return Ok::<(), anyhow::Error>(());
            }

            $body
        }
    }};
}
