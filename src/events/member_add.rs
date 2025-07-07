use twilight_model::gateway::payload::incoming::MemberAdd;
use twilight_model::id::Id;

use crate::configs::discord::{CACHE, HTTP};
use crate::send_with_fallback;
use crate::services::spam::SpamService;
use crate::{
    dbs::mongo::{channel::ChannelEnum, role::RoleEnum},
    services::{channel::ChannelService, role::RoleService},
    utils::embed,
};

pub async fn handle(event: MemberAdd) {
    if event.member.user.bot | event.member.user.system.unwrap_or_default() {
        return;
    }
    let guild_id = event.guild_id;

    if let (Some(token), Some(q_role), Some(q_channel)) = (
        SpamService::get_token(guild_id.get(), event.user.id.get()).await,
        RoleService::get_by_type(guild_id.get(), &RoleEnum::Quarantine).await,
        ChannelService::get_by_type(guild_id.get(), &ChannelEnum::Quarantine).await,
    ) {
        let _ = HTTP
            .add_guild_member_role(guild_id, event.user.id, Id::new(q_role.role_id))
            .await;

        if let Some(guild_ref) = CACHE.guild(guild_id) {
            send_with_fallback!(HTTP, event.user.id, Id::new(q_channel.channel_id), |msg| {
                let embed =
                    embed::quarantine_reminder_embed(&guild_ref, q_channel.channel_id, &token)?;
                msg.embeds(&[embed]).await?;
                Ok::<_, anyhow::Error>(())
            });
        }

        return;
    }

    let guest_role = RoleService::get_by_type(guild_id.get(), &RoleEnum::Guest).await;
    let intro_channel =
        ChannelService::get_by_type(guild_id.get(), &ChannelEnum::Introduction).await;

    if let (Some(guest), Some(channel)) = (guest_role, intro_channel) {
        let guest_id = Id::new(guest.role_id);
        let _ = HTTP
            .add_guild_member_role(guild_id, event.user.id, guest_id)
            .await;

        if let Some(guild_ref) = CACHE.guild(guild_id) {
            send_with_fallback!(HTTP, event.user.id, Id::new(channel.channel_id), |msg| {
                let embed = embed::intro_prompt_embed(&guild_ref, channel.channel_id)?;
                msg.embeds(&[embed]).await?;
                Ok::<_, anyhow::Error>(())
            });
        }
    }
}
