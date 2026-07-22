use std::sync::Arc;

use anyhow::Context as _;
use mongodb::bson::doc;
use twilight_model::id::{
    Id,
    marker::{GuildMarker, UserMarker},
};

use crate::{
    context::Context,
    dbs::mongo::models::{channel::Channel, role::RoleEnum},
    send_with_fallback,
    utils::embed::welcome_embed,
};

use super::embed;
use super::form::IntroDetails;

pub async fn handle_valid_intro(
    ctx: &Arc<Context>,
    user_id: Id<UserMarker>,
    guild_id: Id<GuildMarker>,
    intro_channel: &Channel,
    details: &IntroDetails,
    member_tag: &str,
) -> anyhow::Result<()> {
    // Build + validate the embed before mutating any roles, so a validation failure
    // (e.g. text too long) can never leave a user with swapped roles and no posted intro.
    let intro_embed = {
        let guild_ref = ctx
            .cache
            .guild(guild_id)
            .context("no guild")?;
        embed::intro_details_embed(&guild_ref, member_tag, details)?
    };

    let db = ctx.mongo.clone();

    let roles = tokio::try_join!(
        db.roles.find_one(doc! {
            "role_type": RoleEnum::Guest.value(),
            "guild_id": guild_id.get() as i64,
        }),
        db.roles.find_one(doc! {
            "role_type": RoleEnum::Member.value(),
            "guild_id": guild_id.get() as i64,
        }),
    )?;

    if let Some(role_to_add) = roles.1 {
        ctx.http
            .add_guild_member_role(guild_id, user_id, Id::new(role_to_add.role_id))
            .await?;
    }
    if let Some(role_to_remove) = roles.0 {
        ctx.http
            .remove_guild_member_role(guild_id, user_id, Id::new(role_to_remove.role_id))
            .await?;
    }

    if let Some(guild_ref) = ctx.cache.guild(guild_id) {
        send_with_fallback!(
            ctx,
            user_id,
            Id::new(intro_channel.channel_id),
            |msg| {
                let welcome = welcome_embed(&guild_ref, member_tag, &details.name)?;
                msg.embeds(&[welcome]).await?;
                Ok::<_, anyhow::Error>(())
            }
        );
    }
    ctx.http
        .create_message(Id::new(intro_channel.channel_id))
        .embeds(&[intro_embed])
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use twilight_cache_inmemory::DefaultInMemoryCache;
    use twilight_model::{
        gateway::payload::incoming::GuildCreate,
        guild::{
            AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel,
            NSFWLevel, PremiumTier, SystemChannelFlags, VerificationLevel,
        },
        id::marker::{ChannelMarker, RoleMarker},
    };

    use crate::{
        context::{ContextBuilder, mock_http::MockClient},
        dbs::mongo::models::{
            channel::{Channel, ChannelEnum},
            role::Role,
        },
    };

    use super::*;

    fn make_guild(id: Id<GuildMarker>, name: &str) -> Guild {
        Guild {
            afk_channel_id: None,
            afk_timeout: AfkTimeout::FIVE_MINUTES,
            application_id: None,
            approximate_member_count: None,
            approximate_presence_count: None,
            banner: None,
            channels: Vec::new(),
            default_message_notifications: DefaultMessageNotificationLevel::Mentions,
            description: None,
            discovery_splash: None,
            emojis: Vec::new(),
            explicit_content_filter: ExplicitContentFilter::None,
            features: Vec::new(),
            guild_scheduled_events: Vec::new(),
            icon: None,
            id,
            joined_at: None,
            large: false,
            max_members: None,
            max_presences: None,
            max_stage_video_channel_users: None,
            max_video_channel_users: None,
            member_count: None,
            members: Vec::new(),
            mfa_level: MfaLevel::None,
            name: name.to_owned(),
            nsfw_level: NSFWLevel::Default,
            owner_id: Id::new(1),
            owner: None,
            permissions: None,
            preferred_locale: "en_us".to_owned(),
            premium_progress_bar_enabled: false,
            premium_subscription_count: None,
            premium_tier: PremiumTier::None,
            presences: Vec::new(),
            public_updates_channel_id: None,
            roles: Vec::new(),
            rules_channel_id: None,
            safety_alerts_channel_id: None,
            splash: None,
            stage_instances: Vec::new(),
            stickers: Vec::new(),
            system_channel_flags: SystemChannelFlags::empty(),
            system_channel_id: None,
            threads: Vec::new(),
            unavailable: Some(false),
            vanity_url_code: None,
            verification_level: VerificationLevel::None,
            voice_states: Vec::new(),
            widget_channel_id: None,
            widget_enabled: None,
        }
    }

    #[tokio::test]
    async fn failed_member_role_add_leaves_guest_role_in_place() {
        let guild_id = Id::new(1);
        let user_id = Id::new(42);
        let guest_role_id = Id::new(10);
        let member_role_id: Id<RoleMarker> = Id::new(20);

        let cache = DefaultInMemoryCache::new();
        cache.update(&GuildCreate::Available(make_guild(
            guild_id, "guild",
        )));

        let http = MockClient::new();
        http.set_member_roles(guild_id, user_id, vec![guest_role_id]);
        http.fail_next_add_guild_member_role();

        let ctx = Arc::new(
            ContextBuilder::new()
                .http(http)
                .cache(cache)
                .watchers(false)
                .build()
                .await
                .expect("failed to build Context"),
        );

        ctx.mongo
            .roles
            .insert_one(Role {
                role_type: RoleEnum::Guest,
                role_id: guest_role_id.get(),
                guild_id: guild_id.get(),
                ..Role::default()
            })
            .await
            .expect("failed to insert guest role");
        ctx.mongo
            .roles
            .insert_one(Role {
                role_type: RoleEnum::Member,
                role_id: member_role_id.get(),
                guild_id: guild_id.get(),
                ..Role::default()
            })
            .await
            .expect("failed to insert member role");

        let intro_channel = Channel {
            channel_type: ChannelEnum::Introduction,
            channel_id: Id::<ChannelMarker>::new(30).get(),
            guild_id: guild_id.get(),
            ..Channel::default()
        };
        let details = IntroDetails { name: "Alice".to_owned(), age: None, ign: None, clan: None };

        let result = handle_valid_intro(
            &ctx,
            user_id,
            guild_id,
            &intro_channel,
            &details,
            "alice",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(
            ctx.http.member_roles(guild_id, user_id),
            vec![guest_role_id]
        );
    }
}
