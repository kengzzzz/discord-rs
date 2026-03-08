use crate::{
    context::Context, dbs::mongo::models::channel::ChannelEnum, services::channel::ChannelService,
};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use twilight_model::{
    channel::message::embed::Embed, gateway::payload::incoming::VoiceStateUpdate, id::Id,
};

pub async fn handle(ctx: Arc<Context>, event: VoiceStateUpdate) {
    let voice_state = &event.0;
    let user_id = voice_state.user_id;
    let channel_id = voice_state.channel_id;

    if let Some(guild_id) = voice_state.guild_id
        && let Some(ch) = ChannelService::get_by_type(&ctx, guild_id.get(), &ChannelEnum::Log).await
    {
        let log_channel_id = Id::new(ch.channel_id);

        let member = voice_state.member.as_ref();

        let username = member
            .map(|m| m.user.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        let display_name = member
            .and_then(|m| m.nick.as_ref())
            .map(|nick| format!("{} ({})", nick, username))
            .unwrap_or(username);

        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let (title_text, color, channel_str) = match channel_id {
            Some(cid) => (
                "Joined Voice Channel",
                0x43B581,
                format!("**Channel:** <#{}>\n", cid.get()),
            ),
            None => ("Left Voice Channel", 0xF04747, String::new()),
        };

        let description = format!(
            "**User:** {} (<@{}>)\n{}**Time:** <t:{}:T> (<t:{}:R>)",
            display_name,
            user_id.get(),
            channel_str,
            now_ts,
            now_ts
        );

        let embed = Embed {
            author: None,
            color: Some(color),
            description: Some(description),
            fields: vec![],
            footer: None,
            image: None,
            kind: "rich".to_string(),
            provider: None,
            thumbnail: None,
            timestamp: None,
            title: Some(title_text.to_string()),
            url: None,
            video: None,
        };

        if let Err(e) = ctx
            .http
            .create_message(log_channel_id)
            .embeds(&[embed])
            .await
        {
            tracing::warn!(error = ?e, "Failed to send voice state log embed");
        }
    }
}
