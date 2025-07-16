pub mod cache;
pub mod client;
pub mod embed;
pub mod session;

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Instant,
};

use tokio::{sync::RwLock, time::sleep};

use once_cell::sync::Lazy;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    application::interaction::{Interaction, message_component::MessageComponentInteractionData},
    channel::message::{
        Embed,
        component::{ActionRow, Button, ButtonStyle, Component},
    },
    http::interaction::{InteractionResponse, InteractionResponseType},
    id::{
        Id,
        marker::{GuildMarker, MessageMarker},
    },
};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::context::Context;

pub use session::{MarketSession, OrderInfo};

static SESSIONS: Lazy<RwLock<HashMap<Id<MessageMarker>, MarketSession>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Clone, Copy)]
pub enum MarketKind {
    Buy,
    Sell,
}

impl MarketKind {
    fn target_type(&self) -> &str {
        match self {
            MarketKind::Buy => "sell",
            MarketKind::Sell => "buy",
        }
    }

    fn action(&self) -> &str {
        match self {
            MarketKind::Buy => "buy",
            MarketKind::Sell => "sell",
        }
    }

    fn label(&self) -> &str {
        match self {
            MarketKind::Buy => "ผู้ขาย",
            MarketKind::Sell => "ผู้ซื้อ",
        }
    }
}

pub struct MarketService;

impl MarketService {
    const SESSION_TTL: Duration = Duration::from_secs(900);

    fn spawn_expiration(message_id: Id<MessageMarker>, token: CancellationToken) {
        tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => {},
                _ = sleep(Self::SESSION_TTL) => {
                    SESSIONS.write().await.remove(&message_id);
                }
            }
        });
    }

    pub fn embed_for_session(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        session: &MarketSession,
    ) -> anyhow::Result<Embed> {
        let orders = session.slice().to_vec();
        Self::build_embed(
            guild,
            &session.item,
            &session.url,
            &session.kind,
            session.max_rank.map(|_| session.rank),
            orders,
        )
    }

    pub async fn create_session(
        ctx: Arc<Context>,
        item: &str,
        kind: MarketKind,
    ) -> anyhow::Result<Option<MarketSession>> {
        let Some(url) = Self::find_url(item).await else {
            return Ok(None);
        };
        match client::fetch_orders_map(ctx.reqwest.as_ref(), &url, &kind).await {
            Ok((orders, max_rank)) => {
                if orders.is_empty() {
                    return Ok(None);
                }
                let session = MarketSession {
                    item: item.to_string(),
                    url,
                    kind,
                    orders,
                    rank: 0,
                    page: 1,
                    max_rank,
                    last_used: Instant::now(),
                    expire_token: CancellationToken::new(),
                };
                Ok(Some(session))
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to fetch market orders");
                Ok(None)
            }
        }
    }

    pub fn components(session: &MarketSession) -> Vec<Component> {
        let mut buttons = Vec::with_capacity(5);
        buttons.push(Component::Button(Button {
            custom_id: Some("market_prev_page".into()),
            disabled: session.page <= 1,
            emoji: None,
            label: Some("ก่อนหน้า".into()),
            style: ButtonStyle::Primary,
            url: None,
            sku_id: None,
        }));
        buttons.push(Component::Button(Button {
            custom_id: Some("market_next_page".into()),
            disabled: session.page >= session.lpage(),
            emoji: None,
            label: Some("ถัดไป".into()),
            style: ButtonStyle::Primary,
            url: None,
            sku_id: None,
        }));
        if let Some(max) = session.max_rank {
            buttons.push(Component::Button(Button {
                custom_id: Some("market_next_rank".into()),
                disabled: session.rank >= max,
                emoji: None,
                label: Some("เพิ่ม Rank".into()),
                style: ButtonStyle::Primary,
                url: None,
                sku_id: None,
            }));
            buttons.push(Component::Button(Button {
                custom_id: Some("market_prev_rank".into()),
                disabled: session.rank == 0,
                emoji: None,
                label: Some("ลด Rank".into()),
                style: ButtonStyle::Primary,
                url: None,
                sku_id: None,
            }));
        }
        buttons.push(Component::Button(Button {
            custom_id: Some("market_refresh".into()),
            disabled: false,
            emoji: None,
            label: Some("รีโหลด".into()),
            style: ButtonStyle::Primary,
            url: None,
            sku_id: None,
        }));
        vec![Component::ActionRow(ActionRow {
            components: buttons,
        })]
    }

    pub async fn insert_session(message_id: Id<MessageMarker>, session: MarketSession) {
        let mut session = session;
        session.touch();
        let token = session.expire_token.clone();
        SESSIONS.write().await.insert(message_id, session);
        Self::spawn_expiration(message_id, token);
    }

    async fn get_session_mut(message_id: Id<MessageMarker>) -> Option<MarketSession> {
        SESSIONS.write().await.remove(&message_id).map(|mut s| {
            s.touch();
            s
        })
    }

    async fn store_session(message_id: Id<MessageMarker>, session: MarketSession) {
        let token = session.expire_token.clone();
        SESSIONS.write().await.insert(message_id, session);
        Self::spawn_expiration(message_id, token);
    }

    async fn refresh(ctx: Arc<Context>, session: &mut MarketSession) {
        if let Ok((orders, max)) =
            client::fetch_orders_map(ctx.reqwest.as_ref(), &session.url, &session.kind).await
        {
            if !orders.is_empty() {
                session.orders = orders;
                session.max_rank = max;
                session.page = 1;
                session.rank = session.rank.min(max.unwrap_or(0));
            }
        }
    }

    pub async fn handle_component(
        ctx: Arc<Context>,
        interaction: Interaction,
        data: MessageComponentInteractionData,
    ) {
        let Some(message) = interaction.message else {
            return;
        };
        let message_id = message.id;
        let Some(mut session) = Self::get_session_mut(message_id).await else {
            return;
        };

        match data.custom_id.as_str() {
            "market_prev_page" => {
                if session.page > 1 {
                    session.page -= 1;
                }
            }
            "market_next_page" => {
                if session.page < session.lpage() {
                    session.page += 1;
                }
            }
            "market_next_rank" => {
                if let Some(max) = session.max_rank {
                    if session.rank < max {
                        session.rank += 1;
                        session.page = 1;
                    }
                }
            }
            "market_prev_rank" => {
                if session.rank > 0 {
                    session.rank -= 1;
                    session.page = 1;
                }
            }
            "market_refresh" => {
                Self::refresh(ctx.clone(), &mut session).await;
            }
            _ => {}
        }

        if let Some(guild_ref) = interaction.guild_id.and_then(|id| ctx.cache.guild(id)) {
            if let Ok(embed) = Self::embed_for_session(&guild_ref, &session) {
                let components = Self::components(&session);
                let data = InteractionResponseDataBuilder::new()
                    .embeds([embed])
                    .components(components.clone())
                    .build();
                let http = ctx.http.clone();
                if let Err(e) = http
                    .interaction(interaction.application_id)
                    .create_response(
                        interaction.id,
                        &interaction.token,
                        &InteractionResponse {
                            kind: InteractionResponseType::UpdateMessage,
                            data: Some(data),
                        },
                    )
                    .await
                {
                    tracing::warn!(error = %e, "failed to update market session message");
                }
            }
        }

        Self::store_session(message_id, session).await;
    }

    pub async fn market_embed(
        ctx: Arc<Context>,
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
        kind: MarketKind,
    ) -> anyhow::Result<Embed> {
        let Some(url) = Self::find_url(item).await else {
            return Self::not_found_embed(guild);
        };
        match client::fetch_orders(ctx.reqwest.as_ref(), &url).await {
            Ok(orders) => {
                let mut by_rank: BTreeMap<u8, Vec<session::OrderInfo>> = BTreeMap::new();
                for o in orders {
                    if o.user.status != "ingame" || o.order_type == kind.action() {
                        continue;
                    }
                    let rank = o.mod_rank.unwrap_or(0);
                    by_rank.entry(rank).or_default().push(session::OrderInfo {
                        quantity: o.quantity,
                        platinum: o.platinum,
                        ign: o.user.ingame_name,
                    });
                }
                if by_rank.is_empty() {
                    return Self::not_found_embed(guild);
                }
                for vec in by_rank.values_mut() {
                    if kind.target_type() == "sell" {
                        vec.sort_unstable_by_key(|o| o.platinum);
                    } else {
                        vec.sort_unstable_by(|a, b| b.platinum.cmp(&a.platinum));
                    }
                }
                let Some((&rank, orders)) = by_rank.iter().next() else {
                    return Self::not_found_embed(guild);
                };
                Self::build_embed(
                    guild,
                    item,
                    &url,
                    &kind,
                    if by_rank.len() > 1 { Some(rank) } else { None },
                    orders.clone(),
                )
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to fetch market orders");
                Self::error_embed(guild)
            }
        }
    }
}
