use chrono::Utc;
use deadpool_redis::Pool;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::channel::message::{Embed, embed::EmbedField};
use twilight_model::id::{Id, marker::GuildMarker};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder, ImageSource};

use std::{future::Future, sync::Arc};

use super::api;
use super::utils::{format_time, title_case};
use crate::configs::{CACHE_PREFIX, Reaction};
use crate::context::Context;
use crate::dbs::redis::{redis_get, redis_set_ex};
use crate::utils::embed::footer_with_icon;
use serde::{Serialize, de::DeserializeOwned};

const COLOR: u32 = 0xF1C40F;
const URL: &str = "https://github.com/kengzzzz/discord-rs";
const MIN_CACHE_TTL: usize = 60;

fn ttl_from_expiry(expiry: &str) -> usize {
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(expiry) {
        let secs = t.with_timezone(&Utc).timestamp() - Utc::now().timestamp();
        std::cmp::max(secs.max(0) as usize, MIN_CACHE_TTL)
    } else {
        MIN_CACHE_TTL
    }
}

async fn cached_or_request<T, F, Fut, G>(
    pool: &Pool,
    key: &str,
    fetcher: F,
    ttl_calc: G,
) -> anyhow::Result<T>
where
    T: Serialize + DeserializeOwned + Send + Sync,
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
    G: Fn(&T) -> usize,
{
    if let Some(val) = redis_get(pool, key).await {
        return Ok(val);
    }
    let val = fetcher().await?;
    let ttl = ttl_calc(&val);
    redis_set_ex(pool, key, &val, ttl).await;
    Ok(val)
}

async fn image_link(ctx: &Arc<Context>) -> anyhow::Result<Option<String>> {
    let key = format!("{CACHE_PREFIX}:wf:news");
    let client = ctx.reqwest.clone();
    match cached_or_request(
        &ctx.redis,
        &key,
        move || async move { api::news(&client).await },
        |_| MIN_CACHE_TTL,
    )
    .await
    {
        Ok(data) => Ok(data.last().and_then(|i| i.image_link.clone())),
        Err(e) => {
            tracing::warn!(error = %e, "failed to fetch news image");
            Ok(None)
        }
    }
}

async fn cycle_field(ctx: &Arc<Context>, endpoint: &str, name: &str) -> anyhow::Result<EmbedField> {
    let key = format!("{CACHE_PREFIX}:wf:cycle:{endpoint}");
    let client = ctx.reqwest.clone();
    let data = cached_or_request(
        &ctx.redis,
        &key,
        move || async move { api::cycle(&client, endpoint).await },
        |d| ttl_from_expiry(&d.expiry),
    )
    .await?;
    let field = EmbedFieldBuilder::new(
        format!(
            "{}{}{}",
            Reaction::Load.emoji(),
            name,
            Reaction::Load.emoji()
        ),
        format!("{}\n{}", title_case(&data.state), format_time(&data.expiry)),
    )
    .inline()
    .build();
    Ok(field)
}

pub async fn steel_path_field(ctx: &Arc<Context>) -> anyhow::Result<(EmbedField, bool)> {
    let key = format!("{CACHE_PREFIX}:wf:steel-path");
    let client = ctx.reqwest.clone();
    let data = cached_or_request(
        &ctx.redis,
        &key,
        move || async move { api::steel_path(&client).await },
        |d| ttl_from_expiry(&d.expiry),
    )
    .await?;
    let mut is_umbra = false;
    if let Some(reward) = &data.current_reward {
        if reward.name == "Umbra Forma Blueprint" {
            if let Some(act) = &data.activation {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(act) {
                    let diff = (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
                        .num_minutes()
                        .abs();
                    is_umbra = diff <= 5;
                }
            }
        } else {
            is_umbra = false;
        }
    }
    let value = format!(
        "**{}**\nends {}",
        data.current_reward.map(|r| r.name).unwrap_or_default(),
        format_time(&data.expiry)
    );
    let field = EmbedFieldBuilder::new(
        format!(
            "{}Steel Path{}",
            Reaction::Load.emoji(),
            Reaction::Load.emoji()
        ),
        value,
    )
    .inline()
    .build();
    Ok((field, is_umbra))
}

pub async fn status_embed(
    ctx: &Arc<Context>,
    guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
) -> anyhow::Result<(Embed, bool)> {
    let image_fut = image_link(ctx);
    let steel_fut = steel_path_field(ctx);
    let earth_fut = cycle_field(ctx, "earthCycle", "Earth");
    let cetus_fut = cycle_field(ctx, "cetusCycle", "Cetus");
    let vallis_fut = cycle_field(ctx, "vallisCycle", "Vallis");
    let cambion_fut = cycle_field(ctx, "cambionCycle", "Cambion");
    let zariman_fut = cycle_field(ctx, "zarimanCycle", "Zariman");

    let (image, (steel, is_umbra), earth, cetus, vallis, cambion, zariman) = tokio::try_join!(
        image_fut,
        steel_fut,
        earth_fut,
        cetus_fut,
        vallis_fut,
        cambion_fut,
        zariman_fut
    )?;

    let mut builder = EmbedBuilder::new()
        .title("[PC] Warframe Cycle Timers")
        .url(URL)
        .color(COLOR)
        .field(steel)
        .field(earth)
        .field(cetus)
        .field(vallis)
        .field(cambion)
        .field(zariman)
        .timestamp(twilight_model::util::Timestamp::from_micros(
            Utc::now().timestamp_micros(),
        )?);

    if let Some(img) = image {
        if let Ok(img_src) = ImageSource::url(&img) {
            builder = builder.image(img_src);
        }
    }

    let mut footer = footer_with_icon(guild)?;
    footer.text = guild.name().to_string();

    let embed = builder.footer(footer).build();
    Ok((embed, is_umbra))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        configs::CACHE_PREFIX,
        context::ContextBuilder,
        dbs::{
            mongo::MongoDB,
            redis::{new_pool, redis_set_ex},
        },
        warframe::api::{SteelPathData, SteelPathReward},
    };
    use std::sync::Arc;
    use tokio::sync::OnceCell;
    use twilight_cache_inmemory::DefaultInMemoryCache;
    use twilight_model::gateway::payload::incoming::GuildCreate;
    use twilight_model::guild::{
        AfkTimeout, DefaultMessageNotificationLevel, ExplicitContentFilter, Guild, MfaLevel,
        NSFWLevel, PremiumTier, SystemChannelFlags, VerificationLevel,
    };

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

    async fn build_context() -> Arc<Context> {
        static CTX: OnceCell<Arc<Context>> = OnceCell::const_new();
        CTX.get_or_init(|| async {
            unsafe {
                std::env::set_var("REDIS_URL", "redis://127.0.0.1:6379");
            }
            let http = twilight_http::Client::new("test".into());
            let cache = DefaultInMemoryCache::new();
            let redis = new_pool();
            let mongo = MongoDB::init(redis.clone(), false).await.unwrap();
            let reqwest = reqwest::Client::new();
            Arc::new(Context {
                http,
                cache,
                redis,
                mongo,
                reqwest,
            })
        })
        .await
        .clone()
    }

    #[test]
    fn test_ttl_from_expiry_minimum() {
        let expiry = (Utc::now() + chrono::Duration::seconds(10)).to_rfc3339();
        let ttl = ttl_from_expiry(&expiry);
        assert!(ttl >= MIN_CACHE_TTL);
    }

    #[test]
    fn test_ttl_from_expiry_invalid() {
        assert_eq!(ttl_from_expiry("invalid"), MIN_CACHE_TTL);
    }

    #[tokio::test]
    async fn test_steel_path_field_detects_umbra() {
        let ctx = build_context().await;
        let expiry = (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
        let activation = Utc::now().to_rfc3339();
        let data = api::SteelPathData {
            current_reward: Some(api::SteelPathReward {
                name: "Umbra Forma Blueprint".into(),
            }),
            expiry: expiry.clone(),
            activation: Some(activation),
        };
        let key = format!("{CACHE_PREFIX}:wf:steel-path");
        redis_set_ex(&ctx.redis, &key, &data, 60).await;

        let (field, is_umbra) = steel_path_field(&ctx).await.unwrap();
        assert!(is_umbra);
        let expected_value = format!("**Umbra Forma Blueprint**\nends {}", format_time(&expiry));
        assert_eq!(field.value, expected_value);
    }

    #[tokio::test]
    async fn test_status_embed_footer_and_fields() {
        let ctx = build_context().await;
        let exp = (Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();

        let news_key = format!("{CACHE_PREFIX}:wf:news");
        let news = vec![api::NewsItem {
            image_link: Some("https://example.com/img.png".into()),
        }];
        redis_set_ex(&ctx.redis, &news_key, &news, 60).await;

        let steel_key = format!("{CACHE_PREFIX}:wf:steel-path");
        let steel = api::SteelPathData {
            current_reward: Some(api::SteelPathReward {
                name: "Umbra Forma Blueprint".into(),
            }),
            expiry: exp.clone(),
            activation: Some(Utc::now().to_rfc3339()),
        };
        redis_set_ex(&ctx.redis, &steel_key, &steel, 60).await;

        let cycle_endpoints = [
            ("earthCycle", "Earth"),
            ("cetusCycle", "Cetus"),
            ("vallisCycle", "Vallis"),
            ("cambionCycle", "Cambion"),
            ("zarimanCycle", "Zariman"),
        ];
        for (ep, _) in &cycle_endpoints {
            let key = format!("{CACHE_PREFIX}:wf:cycle:{ep}");
            let cycle = api::Cycle {
                state: "state".into(),
                expiry: exp.clone(),
            };
            redis_set_ex(&ctx.redis, &key, &cycle, 60).await;
        }

        let guild = make_guild(Id::new(1), "guild");
        let cache = DefaultInMemoryCache::new();
        cache.update(&GuildCreate::Available(guild.clone()));
        let guild_ref = cache.guild(guild.id).expect("guild ref");

        let (embed, is_umbra) = status_embed(&ctx, &guild_ref).await.unwrap();
        assert!(is_umbra);
        assert_eq!(embed.title.as_deref(), Some("[PC] Warframe Cycle Timers"));
        assert_eq!(embed.footer.unwrap().text, "guild");
        assert_eq!(embed.fields.len(), 6);
    }

    #[tokio::test]
    async fn test_ttl_from_expiry_future() {
        let expiry = (Utc::now() + chrono::Duration::seconds(120)).to_rfc3339();
        assert!(ttl_from_expiry(&expiry) >= 120);
    }

    #[tokio::test]
    async fn test_steel_path_field_umbra() {
        let ctx = Arc::new(ContextBuilder::new().watchers(false).build().await.unwrap());
        let data = SteelPathData {
            current_reward: Some(SteelPathReward {
                name: "Umbra Forma Blueprint".to_string(),
            }),
            expiry: (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            activation: Some((Utc::now() - chrono::Duration::minutes(2)).to_rfc3339()),
        };
        let key = format!("{CACHE_PREFIX}:wf:steel-path");
        redis_set_ex(&ctx.redis, &key, &data, 60).await;

        let (_field, is_umbra) = steel_path_field(&ctx).await.unwrap();
        assert!(is_umbra);
    }
}
