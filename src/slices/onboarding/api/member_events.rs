use std::sync::Arc;

use async_trait::async_trait;
use twilight_model::{
    gateway::payload::incoming::{MemberAdd, MemberRemove},
    id::Id,
};

use crate::{
    context::Context,
    dbs::mongo::models::{channel::ChannelEnum, role::RoleEnum},
    send_with_fallback,
    services::{channel::ChannelService, introduction, role::RoleService, spam},
    slices::onboarding::{
        app::{member_flow::plan_member_join, ports::OnboardingReadPorts},
        domain::{ChannelKind, JoinPlan, RoleKind},
    },
};

struct ContextPorts<'a> {
    ctx: &'a Arc<Context>,
}

#[async_trait]
impl OnboardingReadPorts for ContextPorts<'_> {
    async fn quarantine_token(&self, guild_id: u64, user_id: u64) -> Option<String> {
        spam::quarantine::get_token(self.ctx, guild_id, user_id).await
    }

    async fn role_id(&self, guild_id: u64, role: RoleKind) -> Option<u64> {
        let role_kind = match role {
            RoleKind::Guest => RoleEnum::Guest,
            RoleKind::Quarantine => RoleEnum::Quarantine,
        };

        RoleService::get_by_type(self.ctx, guild_id, &role_kind)
            .await
            .map(|role| role.role_id)
    }

    async fn channel_id(&self, guild_id: u64, channel: ChannelKind) -> Option<u64> {
        let channel_kind = match channel {
            ChannelKind::Introduction => ChannelEnum::Introduction,
            ChannelKind::Quarantine => ChannelEnum::Quarantine,
        };

        ChannelService::get_by_type(self.ctx, guild_id, &channel_kind)
            .await
            .map(|channel| channel.channel_id)
    }
}

pub async fn handle_member_add(ctx: Arc<Context>, event: MemberAdd) {
    let guild_id = event.guild_id;
    let user = &event.user;
    let plan = plan_member_join(
        &ContextPorts { ctx: &ctx },
        guild_id.get(),
        user.id.get(),
        event.member.user.bot,
        event
            .member
            .user
            .system
            .unwrap_or_default(),
    )
    .await;

    match plan {
        JoinPlan::Ignore | JoinPlan::Noop => {}
        JoinPlan::RestoreQuarantine { token, role_id, channel_id } => {
            if let Err(error) = ctx
                .http
                .add_guild_member_role(guild_id, user.id, Id::new(role_id))
                .await
            {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    user_id = user.id.get(),
                    error = %error,
                    "failed to assign quarantine role on join"
                );
            }

            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                send_with_fallback!(ctx, user.id, Id::new(channel_id), |msg| {
                    let embed =
                        spam::embed::quarantine_reminder_embed(&guild_ref, channel_id, &token)?;
                    msg.embeds(&[embed]).await?;
                    Ok::<_, anyhow::Error>(())
                });
            }
        }
        JoinPlan::AssignGuest { role_id, channel_id } => {
            if let Err(error) = ctx
                .http
                .add_guild_member_role(guild_id, user.id, Id::new(role_id))
                .await
            {
                tracing::warn!(
                    guild_id = guild_id.get(),
                    user_id = user.id.get(),
                    error = %error,
                    "failed to assign guest role on join"
                );
            }

            if let Some(guild_ref) = ctx.cache.guild(guild_id) {
                send_with_fallback!(ctx, user.id, Id::new(channel_id), |msg| {
                    let embed = introduction::embed::intro_prompt_embed(&guild_ref, channel_id)?;
                    msg.embeds(&[embed]).await?;
                    Ok::<_, anyhow::Error>(())
                });
            }
        }
    }
}

pub async fn handle_member_remove(ctx: Arc<Context>, event: MemberRemove) {
    if event.user.bot || event.user.system.unwrap_or_default() {
        return;
    }

    spam::log::clear_log(
        &ctx.redis,
        event.guild_id.get(),
        event.user.id.get(),
    )
    .await;
}
