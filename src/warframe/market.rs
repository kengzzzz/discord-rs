use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use twilight_model::{
    application::interaction::Interaction,
    channel::message::{Component, component::ButtonStyle},
    id::{
        Id,
        marker::{MessageMarker, UserMarker},
    },
};

use crate::{
    app::App,
    db::{self, catalog},
    discord::{
        reply,
        ui::{self, Card, color},
    },
};

const API: &str = "https://api.warframe.market/v2";
const ASSETS: &str = "https://warframe.market/static/assets/";
const CATALOG: &str = "wfm_items";
const PAGE_SIZE: usize = 5;
const SESSION_IDLE: Duration = Duration::from_secs(15 * 60);
/// Each session holds its own copy of the orders for 15 minutes.
const SESSIONS_PER_MEMBER: usize = 3;
const MAX_SESSIONS: usize = 100;
const ORDER_CACHE: Duration = Duration::from_secs(45);
const MAX_CACHED_ORDERS: usize = 5000;
const REFRESH_MIN_AGE: Duration = Duration::from_secs(10);
pub const PREFIX: &str = "mkt:";

type OrderCache = HashMap<String, (Instant, Arc<Vec<Order>>)>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub max_rank: Option<u8>,
    #[serde(default)]
    pub subtypes: Vec<String>,
    #[serde(default)]
    pub thumb: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Buy,
    Sell,
}

impl Kind {
    const fn wanted_order(self) -> &'static str {
        match self {
            Self::Buy => "sell",
            Self::Sell => "buy",
        }
    }

    const fn verb(self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Buy => "ผู้ขาย",
            Self::Sell => "ผู้ซื้อ",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub platinum: u32,
    pub quantity: u32,
    pub rank: Option<u8>,
    pub subtype: Option<String>,
    pub ign: String,
    pub reputation: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Variant {
    pub rank: Option<u8>,
    pub subtype: Option<String>,
}

pub struct Session {
    pub item: Item,
    pub kind: Kind,
    pub orders: Vec<Order>,
    pub variants: Vec<Variant>,
    pub variant: usize,
    pub page: usize,
    pub fetched_at: i64,
    last_used: Instant,
    owner: Option<Id<UserMarker>>,
}

impl Session {
    pub fn new(item: Item, kind: Kind, orders: Vec<Order>) -> Self {
        let mut session = Self {
            item,
            kind,
            orders: Vec::new(),
            variants: Vec::new(),
            variant: 0,
            page: 0,
            fetched_at: 0,
            last_used: Instant::now(),
            owner: None,
        };
        session.set_orders(orders);
        session
    }

    fn set_orders(&mut self, mut orders: Vec<Order>) {
        let kind = self.kind;
        orders.sort_by(|a, b| match kind {
            Kind::Buy => a.platinum.cmp(&b.platinum),
            Kind::Sell => b.platinum.cmp(&a.platinum),
        });
        let previous = self.variants.get(self.variant).cloned();
        let mut variants: Vec<Variant> = orders
            .iter()
            .map(|o| Variant { rank: o.rank, subtype: o.subtype.clone() })
            .collect();
        let order_of = |v: &Variant| {
            v.subtype.as_ref().and_then(|s| {
                self.item
                    .subtypes
                    .iter()
                    .position(|x| x == s)
            })
        };
        variants.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then(order_of(a).cmp(&order_of(b)))
                .then(a.subtype.cmp(&b.subtype))
        });
        variants.dedup();
        self.variant = previous
            .and_then(|p| variants.iter().position(|v| *v == p))
            .unwrap_or(0);
        self.variants = variants;
        self.orders = orders;
        self.page = 0;
        self.fetched_at = jiff::Timestamp::now().as_millisecond();
    }

    pub fn visible(&self) -> Vec<&Order> {
        let current = self.variants.get(self.variant);
        self.orders
            .iter()
            .filter(|o| current.is_none_or(|v| o.rank == v.rank && o.subtype == v.subtype))
            .collect()
    }

    pub fn pages(&self) -> usize {
        self.visible()
            .len()
            .div_ceil(PAGE_SIZE)
            .max(1)
    }
}

#[derive(Default)]
pub struct Market {
    items: RwLock<Arc<Catalog>>,
    version: Mutex<Option<String>>,
    sessions: Mutex<HashMap<Id<MessageMarker>, Session>>,
    orders: Mutex<OrderCache>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Deserialize)]
struct Versions {
    collections: Collections,
}

#[derive(Deserialize)]
struct Collections {
    items: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawItem {
    slug: String,
    #[serde(default)]
    max_rank: Option<u8>,
    #[serde(default)]
    subtypes: Vec<String>,
    i18n: HashMap<String, RawI18n>,
}

#[derive(Deserialize)]
struct RawI18n {
    name: String,
    #[serde(default)]
    thumb: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOrder<'a> {
    #[serde(rename = "type", borrow)]
    kind: Cow<'a, str>,
    platinum: u32,
    quantity: u32,
    #[serde(default)]
    rank: Option<u8>,
    #[serde(default, borrow)]
    subtype: Option<Cow<'a, str>>,
    #[serde(default = "visible_default")]
    visible: bool,
    #[serde(borrow)]
    user: RawUser<'a>,
}

fn visible_default() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUser<'a> {
    #[serde(borrow)]
    ingame_name: Cow<'a, str>,
    #[serde(borrow)]
    status: Cow<'a, str>,
    #[serde(default)]
    reputation: i64,
}

/// Keeps only the orders shown while parsing, without collecting them all first.
struct KeptOrders(Kind);

impl<'de> Visitor<'de> for KeptOrders {
    type Value = Vec<Order>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a warframe.market orders response")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<Order>, A::Error> {
        let mut kept = None;
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            if key == "data" {
                kept = Some(map.next_value_seed(OrderList(self.0))?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        kept.ok_or_else(|| serde::de::Error::missing_field("data"))
    }
}

struct OrderList(Kind);

const MAX_KEPT_ORDERS: usize = 1000;

impl<'de> DeserializeSeed<'de> for OrderList {
    type Value = Vec<Order>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<Order>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for OrderList {
    type Value = Vec<Order>;

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a list of orders")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Order>, A::Error> {
        let wanted = self.0.wanted_order();
        let mut kept = Vec::new();
        while let Some(o) = seq.next_element::<RawOrder<'de>>()? {
            if o.visible && o.user.status == "ingame" && o.kind == wanted {
                if kept.len() == MAX_KEPT_ORDERS {
                    return Err(serde::de::Error::custom(
                        "too many orders to show",
                    ));
                }
                kept.push(Order {
                    platinum: o.platinum,
                    quantity: o.quantity,
                    rank: o.rank,
                    subtype: o.subtype.map(Cow::into_owned),
                    ign: o.user.ingame_name.into_owned(),
                    reputation: o.user.reputation,
                });
            }
        }
        Ok(kept)
    }
}

pub fn parse_items(body: &[u8]) -> anyhow::Result<Vec<Item>> {
    let envelope: Envelope<Vec<RawItem>> = serde_json::from_slice(body)?;
    let mut items: Vec<Item> = envelope
        .data
        .into_iter()
        .filter_map(|raw| {
            let en = raw.i18n.get("en")?;
            Some(Item {
                name: en.name.clone(),
                slug: raw.slug,
                max_rank: raw.max_rank,
                subtypes: raw.subtypes,
                thumb: en.thumb.clone(),
            })
        })
        .collect();
    items.sort_by_cached_key(|i| i.name.to_lowercase());
    items.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    Ok(items)
}

pub fn parse_orders(body: &[u8], kind: Kind) -> anyhow::Result<Vec<Order>> {
    let mut deserializer = serde_json::Deserializer::from_slice(body);
    let orders = deserializer.deserialize_map(KeptOrders(kind))?;
    deserializer.end()?;
    Ok(orders)
}

#[derive(Default)]
pub struct NameIndex {
    names: Vec<String>,
    keys: Vec<String>,
}

impl NameIndex {
    pub fn new(names: Vec<String>) -> Self {
        let keys = names
            .iter()
            .map(|n| n.to_lowercase())
            .collect();
        Self { names, keys }
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn position(&self, name: &str) -> Option<usize> {
        let key = name.trim().to_lowercase();
        self.keys.iter().position(|k| *k == key)
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<String> {
        let query = query.trim().to_lowercase();
        let mut prefix = Vec::new();
        let mut contains = Vec::new();
        for (name, key) in self.names.iter().zip(&self.keys) {
            if query.is_empty() || key.starts_with(&query) {
                prefix.push(name);
                if prefix.len() >= limit {
                    break;
                }
            } else if contains.len() < limit && key.contains(&query) {
                contains.push(name);
            }
        }
        prefix
            .into_iter()
            .chain(contains)
            .take(limit)
            .cloned()
            .collect()
    }
}

#[derive(Default)]
pub struct Catalog {
    pub items: Vec<Item>,
    index: NameIndex,
}

impl Catalog {
    pub fn new(items: Vec<Item>) -> Self {
        let index = NameIndex::new(
            items
                .iter()
                .map(|i| i.name.clone())
                .collect(),
        );
        Self { items, index }
    }
}

impl Market {
    pub fn items(&self) -> Arc<Catalog> {
        Arc::clone(
            &self
                .items
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    pub fn find(&self, name: &str) -> Option<Item> {
        let catalog = self.items();
        catalog
            .index
            .position(name)
            .and_then(|i| catalog.items.get(i))
            .cloned()
    }

    pub fn complete(&self, query: &str) -> Vec<String> {
        self.items().index.search(query, 25)
    }

    fn set_items(&self, items: Vec<Item>, version: Option<String>) {
        *self
            .items
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(Catalog::new(items));
        *self
            .version
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = version;
    }

    /// Evicts the owner's oldest session past their share, then the oldest overall.
    fn keep(&self, message: Id<MessageMarker>, session: Session) {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        fn oldest(
            sessions: &HashMap<Id<MessageMarker>, Session>,
            pick: impl Fn(&Session) -> bool,
        ) -> Option<Id<MessageMarker>> {
            sessions
                .iter()
                .filter(|(_, s)| pick(s))
                .min_by_key(|(_, s)| s.last_used)
                .map(|(id, _)| *id)
        }
        let owner = session.owner;
        let owned = sessions
            .values()
            .filter(|s| s.owner == owner)
            .count();
        if owned >= SESSIONS_PER_MEMBER
            && let Some(id) = oldest(&sessions, |s| s.owner == owner)
        {
            sessions.remove(&id);
        }
        if sessions.len() >= MAX_SESSIONS
            && let Some(id) = oldest(&sessions, |_| true)
        {
            sessions.remove(&id);
        }
        sessions.insert(message, session);
    }

    pub fn sweep(&self) {
        let now = Instant::now();
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, s| now.duration_since(s.last_used) < SESSION_IDLE);
        self.orders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|_, (at, _)| now.duration_since(*at) < ORDER_CACHE);
    }
}

pub async fn load(app: &App) {
    if let Ok(Some(stored)) = app
        .db
        .call(|c| catalog::get(c, CATALOG))
        .await
        && let Ok(items) = serde_json::from_slice::<Vec<Item>>(&stored.body)
    {
        app.warframe
            .market
            .set_items(items, stored.etag);
    }
}

pub async fn refresh(app: &App) -> anyhow::Result<()> {
    let versions: Envelope<Versions> = app
        .web
        .get(format!("{API}/versions"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let latest = versions.data.collections.items;
    let current = app
        .warframe
        .market
        .version
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if current.as_deref() == Some(latest.as_str())
        && !app
            .warframe
            .market
            .items()
            .items
            .is_empty()
    {
        return Ok(());
    }
    let body = app
        .web
        .get(format!("{API}/items"))
        .header("Language", "en")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let items = parse_items(&body)?;
    // A short list is likely a bad response; don't let it replace the catalog.
    if items.len() < 500 {
        anyhow::bail!(
            "item catalog looks truncated ({} items)",
            items.len()
        );
    }
    let stored = catalog::Stored {
        etag: Some(latest.clone()),
        fetched_at: db::now(),
        body: serde_json::to_vec(&items)?,
    };
    app.db
        .call(move |c| catalog::put(c, CATALOG, &stored))
        .await?;
    tracing::info!(
        count = items.len(),
        "warframe.market catalog updated"
    );
    app.warframe
        .market
        .set_items(items, Some(latest));
    Ok(())
}

async fn fetch_orders(
    app: &App,
    item: &Item,
    kind: Kind,
    fresh: bool,
) -> anyhow::Result<Vec<Order>> {
    let key = format!("{}:{}", item.slug, kind.verb());
    // Refresh bypasses only slightly old data, so spamming it can't hammer warframe.market.
    let max_age = if fresh { REFRESH_MIN_AGE } else { ORDER_CACHE };
    let cached = || {
        app.warframe
            .market
            .orders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .filter(|(at, _)| at.elapsed() < max_age)
            .map(|(_, orders)| orders.as_ref().clone())
    };
    if let Some(orders) = cached() {
        return Ok(orders);
    }
    let _lookup = super::lookup(app).await?;
    // Another search may have fetched it while we waited.
    if let Some(orders) = cached() {
        return Ok(orders);
    }
    let body = super::read_limited(
        app.web
            .get(format!("{API}/orders/item/{}", item.slug))
            .header("Platform", "pc")
            .header("Crossplay", "true"),
    )
    .await?;
    let orders = parse_orders(&body, kind)?;
    cache_orders(
        &mut app
            .warframe
            .market
            .orders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        key,
        Arc::new(orders.clone()),
    );
    Ok(orders)
}

fn cache_orders(cache: &mut OrderCache, key: String, orders: Arc<Vec<Order>>) {
    cache.retain(|k, (at, _)| *k != key && at.elapsed() < ORDER_CACHE);
    let mut total = orders.len()
        + cache
            .values()
            .map(|(_, o)| o.len())
            .sum::<usize>();
    while total > MAX_CACHED_ORDERS {
        let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        if let Some((_, dropped)) = cache.remove(&oldest) {
            total -= dropped.len();
        }
    }
    cache.insert(key, (Instant::now(), orders));
}

fn variant_label(item: &Item, variant: Option<&Variant>) -> String {
    let Some(variant) = variant else { return String::new() };
    let mut parts = Vec::new();
    if let Some(rank) = variant.rank {
        match item.max_rank {
            Some(max) => parts.push(format!("Rank {rank}/{max}")),
            None => parts.push(format!("Rank {rank}")),
        }
    }
    if let Some(subtype) = &variant.subtype {
        parts.push(capitalize(subtype));
    }
    parts.join(" · ")
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn variant_short(variant: &Variant) -> String {
    let mut parts = Vec::new();
    if let Some(rank) = variant.rank {
        parts.push(format!("Rank {rank}"));
    }
    if let Some(subtype) = &variant.subtype {
        parts.push(capitalize(subtype));
    }
    parts.join(" · ")
}

pub fn render(session: &Session) -> Vec<Component> {
    let item = &session.item;
    let visible = session.visible();
    let variant = session.variants.get(session.variant);
    let label = variant_label(item, variant);
    let pages = session.pages();
    let page = session.page.min(pages - 1);
    let mut item_name = item.name.clone();
    if let Some(rank) = variant.and_then(|v| v.rank) {
        item_name.push_str(&format!(" (rank {rank})"));
    }
    // Sellers often list several relic refinements.
    if let Some(subtype) = variant.and_then(|v| v.subtype.as_deref()) {
        item_name.push_str(&format!(" ({})", capitalize(subtype)));
    }

    let mut lines = Vec::new();
    for (index, order) in visible
        .iter()
        .skip(page * PAGE_SIZE)
        .take(PAGE_SIZE)
        .enumerate()
    {
        let ign = order.ign.replace('`', "");
        lines.push(format!(
            "**{}. {}p** × {} · `{ign}` rep {}\n```/w {ign} Hi! I want to {}: \"{}\" for {} platinum. (warframe.market)```",
            page * PAGE_SIZE + index + 1,
            order.platinum,
            order.quantity,
            order.reputation,
            session.kind.verb(),
            item_name.replace('`', ""),
            order.platinum
        ));
    }
    if lines.is_empty() {
        lines.push("ไม่มีผู้เล่นที่ออนไลน์ในเกมตอนนี้".to_owned());
    }

    let thumb = item
        .thumb
        .as_ref()
        .map(|t| format!("{ASSETS}{t}"));
    let mut header = format!(
        "### {} · {}",
        session.kind.label(),
        ui::escape_markdown(&item.name)
    );
    let mut sub = vec![format!("ออนไลน์ในเกม {} รายการ", visible.len())];
    if !label.is_empty() {
        sub.insert(0, label);
    }
    header.push_str(&format!("\n-# {}", sub.join(" · ")));

    let mut card = Card::new(color::BRAND)
        .section(header, thumb.as_deref())
        .text(lines.join("\n"));
    card = card.small(format!(
        "หน้า {}/{pages} · อัปเดต {}",
        page + 1,
        ui::ts(session.fetched_at, 'R')
    ));
    card = card.buttons([
        ui::button_with(
            ButtonStyle::Secondary,
            format!("{PREFIX}prev"),
            "ก่อนหน้า",
            page == 0,
        ),
        ui::button_with(
            ButtonStyle::Secondary,
            format!("{PREFIX}next"),
            "ถัดไป",
            page + 1 >= pages,
        ),
        ui::button_with(
            ButtonStyle::Primary,
            format!("{PREFIX}refresh"),
            "Refresh",
            false,
        ),
        ui::link(
            format!("https://warframe.market/items/{}", item.slug),
            "warframe.market",
        ),
    ]);
    if session.variants.len() > 1 {
        let prev = session.variant.checked_sub(1);
        let next = Some(session.variant + 1).filter(|&i| i < session.variants.len());
        let name = |index: Option<usize>| {
            session
                .variants
                .get(index.unwrap_or(session.variant))
                .map(variant_short)
                .unwrap_or_default()
        };
        card = card.buttons([
            ui::button_with(
                ButtonStyle::Secondary,
                format!("{PREFIX}vprev"),
                format!("‹ {}", name(prev)),
                prev.is_none(),
            ),
            ui::button_with(
                ButtonStyle::Secondary,
                format!("{PREFIX}vnext"),
                format!("{} ›", name(next)),
                next.is_none(),
            ),
        ]);
    }
    vec![card.build()]
}

pub async fn command(
    app: &Arc<App>,
    interaction: &Interaction,
    name: &str,
    kind: Kind,
) -> anyhow::Result<()> {
    reply::defer(app, interaction, true).await?;
    if app
        .warframe
        .market
        .items()
        .items
        .is_empty()
    {
        reply::edit(
            app,
            interaction,
            &ui::error("ข้อมูลตลาดยังไม่พร้อม ลองใหม่ภายหลัง"),
        )
        .await?;
        return Ok(());
    }
    let Some(item) = app.warframe.market.find(name) else {
        let suggestions = app.warframe.market.complete(name);
        let mut body = "ไม่พบไอเทมนี้ เลือกจากรายการตอนพิมพ์".to_owned();
        if !suggestions.is_empty() {
            body.push_str(&format!(
                "\nใกล้เคียง: {}",
                suggestions
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        reply::edit(
            app,
            interaction,
            &ui::notice(color::WARNING, "ไม่พบไอเทม", &body),
        )
        .await?;
        return Ok(());
    };
    let orders = match fetch_orders(app, &item, kind, false).await {
        Ok(orders) => orders,
        Err(error) => {
            super::log_failure("warframe.market", &error);
            reply::edit(
                app,
                interaction,
                &ui::error("ดึงราคาจาก warframe.market ไม่ได้ ลองใหม่ภายหลัง"),
            )
            .await?;
            return Ok(());
        }
    };
    let mut session = Session::new(item, kind, orders);
    session.owner = interaction.author_id();
    let message = reply::edit(app, interaction, &render(&session)).await?;
    app.warframe
        .market
        .keep(message.id, session);
    Ok(())
}

pub async fn on_button(
    app: &Arc<App>,
    interaction: &Interaction,
    custom_id: &str,
) -> anyhow::Result<()> {
    let Some(message) = interaction
        .message
        .as_ref()
        .map(|m| m.id)
    else {
        return Ok(());
    };
    let action = custom_id
        .strip_prefix(PREFIX)
        .unwrap_or_default();
    let exists = app
        .warframe
        .market
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_key(&message);
    if !exists {
        return reply::respond(
            app,
            interaction,
            ui::notice(
                color::MUTED,
                "หมดเวลาแล้ว",
                "ใช้ `/warframe market` อีกครั้ง",
            ),
            true,
        )
        .await;
    }
    if action == "refresh" {
        reply::defer_update(app, interaction).await?;
        let (item, kind) = {
            let sessions = app
                .warframe
                .market
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(session) = sessions.get(&message) else { return Ok(()) };
            (session.item.clone(), session.kind)
        };
        let orders = fetch_orders(app, &item, kind, true).await;
        if let Err(error) = &orders {
            super::log_failure("warframe.market", error);
        }
        let components = {
            let mut sessions = app
                .warframe
                .market
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(session) = sessions.get_mut(&message) else { return Ok(()) };
            session.last_used = Instant::now();
            if let Ok(orders) = orders {
                session.set_orders(orders);
            }
            render(session)
        };
        reply::edit(app, interaction, &components).await?;
        return Ok(());
    }
    let components = {
        let mut sessions = app
            .warframe
            .market
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(session) = sessions.get_mut(&message) else { return Ok(()) };
        session.last_used = Instant::now();
        match action {
            "prev" => session.page = session.page.saturating_sub(1),
            "next" => session.page = (session.page + 1).min(session.pages() - 1),
            "vprev" => {
                session.variant = session.variant.saturating_sub(1);
                session.page = 0;
            }
            "vnext" => {
                session.variant =
                    (session.variant + 1).min(session.variants.len().saturating_sub(1));
                session.page = 0;
            }
            _ => {}
        }
        render(session)
    };
    reply::update(app, interaction, components).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(platinum: u32, rank: Option<u8>, subtype: Option<&str>) -> Order {
        Order {
            platinum,
            quantity: 1,
            rank,
            subtype: subtype.map(str::to_owned),
            ign: format!("p{platinum}"),
            reputation: 0,
        }
    }

    fn item(subtypes: &[&str]) -> Item {
        Item {
            name: "Axi A1 Relic".into(),
            slug: "axi_a1_relic".into(),
            max_rank: None,
            subtypes: subtypes
                .iter()
                .map(|s| (*s).to_owned())
                .collect(),
            thumb: None,
        }
    }

    #[test]
    fn sessions_are_capped_per_member_and_in_total() {
        let market = Market::default();
        let start = Instant::now();
        let add = |n: u64, owner: u64| {
            let mut session = Session::new(item(&[]), Kind::Sell, Vec::new());
            session.owner = Some(Id::new(owner));
            session.last_used = start + Duration::from_millis(n);
            market.keep(Id::new(n), session);
        };
        for n in 1..=4 {
            add(n, 7);
        }
        let ids = |market: &Market| {
            let mut ids: Vec<u64> = market
                .sessions
                .lock()
                .unwrap()
                .keys()
                .map(|id| id.get())
                .collect();
            ids.sort_unstable();
            ids
        };
        assert_eq!(ids(&market), [2, 3, 4]);
        for n in 5..=(MAX_SESSIONS as u64 + 10) {
            add(n, n);
        }
        let kept = ids(&market);
        assert_eq!(kept.len(), MAX_SESSIONS);
        assert_eq!(kept[0], 11);
    }

    #[test]
    fn parses_orders_keeping_only_ingame_opposite_side() {
        let body = serde_json::json!({"apiVersion": "0.25.0", "data": [
            {"type": "sell", "platinum": 10, "quantity": 1, "rank": 0, "visible": true, "user": {"ingameName": "a", "status": "ingame", "reputation": 3}},
            {"type": "sell", "platinum": 5, "quantity": 1, "user": {"ingameName": "b", "status": "online"}},
            {"type": "buy", "platinum": 7, "quantity": 2, "user": {"ingameName": "c", "status": "ingame"}},
            {"type": "sell", "platinum": 1, "quantity": 1, "visible": false, "user": {"ingameName": "d", "status": "ingame"}}
        ], "error": null})
        .to_string();
        let sells = parse_orders(body.as_bytes(), Kind::Buy).unwrap();
        assert_eq!(sells.len(), 1);
        assert_eq!(
            (
                sells[0].ign.as_str(),
                sells[0].rank,
                sells[0].reputation
            ),
            ("a", Some(0), 3)
        );
        assert_eq!(
            parse_orders(body.as_bytes(), Kind::Sell).unwrap()[0].ign,
            "c"
        );
    }

    #[test]
    fn the_order_cache_keeps_a_bounded_number_of_orders() {
        let orders = |n: usize| {
            Arc::new(
                (0..n)
                    .map(|_| Order {
                        platinum: 1,
                        quantity: 1,
                        rank: None,
                        subtype: None,
                        ign: String::new(),
                        reputation: 0,
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let mut cache = OrderCache::new();
        for i in 0..8 {
            cache_orders(&mut cache, format!("item{i}:sell"), orders(1000));
        }
        assert_eq!(cache.len(), MAX_CACHED_ORDERS / 1000);
        assert!(cache.contains_key("item7:sell") && !cache.contains_key("item2:sell"));
        cache_orders(&mut cache, "item7:sell".into(), orders(10));
        assert_eq!(cache["item7:sell"].1.len(), 10);
    }

    #[test]
    fn more_orders_than_budgeted_are_refused() {
        let body = |n: usize| {
            let order = r#"{"type":"sell","platinum":1,"quantity":1,"user":{"ingameName":"a","status":"ingame"}}"#;
            format!("{{\"data\":[{}]}}", vec![order; n].join(","))
        };
        assert_eq!(
            parse_orders(body(MAX_KEPT_ORDERS).as_bytes(), Kind::Buy)
                .unwrap()
                .len(),
            MAX_KEPT_ORDERS
        );
        assert!(parse_orders(body(MAX_KEPT_ORDERS + 1).as_bytes(), Kind::Buy).is_err());
    }

    #[test]
    fn relic_subtypes_are_separate_variants_in_item_order() {
        let session = Session::new(
            item(&["intact", "exceptional", "flawless", "radiant"]),
            Kind::Buy,
            vec![
                order(9, None, Some("radiant")),
                order(2, None, Some("intact")),
                order(3, None, Some("intact")),
            ],
        );
        assert_eq!(session.variants.len(), 2);
        assert_eq!(
            session.variants[0].subtype.as_deref(),
            Some("intact")
        );
        let prices: Vec<u32> = session
            .visible()
            .iter()
            .map(|o| o.platinum)
            .collect();
        assert_eq!(prices, [2, 3]);
        let rendered = serde_json::to_string(&render(&session)).unwrap();
        assert!(
            rendered.contains(r#"(Intact)\" for 2 platinum"#),
            "{rendered}"
        );
    }

    #[test]
    fn sell_kind_sorts_highest_first_and_pages() {
        let orders = (1..=12)
            .map(|p| order(p, Some(0), None))
            .collect();
        let session = Session::new(item(&[]), Kind::Sell, orders);
        assert_eq!(session.visible()[0].platinum, 12);
        assert_eq!(session.pages(), 3);
        ui::validate_message(&render(&session)).unwrap();
        let ranked = Session::new(
            Item {
                max_rank: Some(10),
                name: "Primed Continuity ".repeat(5),
                ..item(&["intact", "radiant"])
            },
            Kind::Buy,
            (0..40)
                .map(|p| {
                    order(
                        p,
                        Some((p % 11) as u8),
                        Some(if p % 2 == 0 { "intact" } else { "radiant" }),
                    )
                })
                .collect(),
        );
        assert!(ranked.variants.len() > 1);
        ui::validate_message(&render(&ranked)).unwrap();
    }

    #[test]
    fn parses_item_catalog_and_searches_prefix_then_substring() {
        let body = serde_json::json!({"data": [
            {"id": "1", "slug": "primed_continuity", "maxRank": 10, "i18n": {"en": {"name": "Primed Continuity", "thumb": "t.png"}}},
            {"id": "2", "slug": "continuity", "maxRank": 5, "i18n": {"en": {"name": "Continuity"}}},
            {"id": "3", "slug": "axi_a1_relic", "subtypes": ["intact", "radiant"], "i18n": {"en": {"name": "Axi A1 Relic"}}}
        ]})
        .to_string();
        let items = parse_items(body.as_bytes()).unwrap();
        assert_eq!(items[0].name, "Axi A1 Relic");
        assert_eq!(
            Catalog::new(items.clone())
                .index
                .search("cont", 5),
            ["Continuity", "Primed Continuity"]
        );
        assert_eq!(
            items
                .iter()
                .find(|i| i.slug == "primed_continuity")
                .unwrap()
                .max_rank,
            Some(10)
        );
    }

    #[test]
    fn name_index_finds_case_insensitively_and_ranks_prefix_first() {
        let index = NameIndex::new(vec![
            "Primed Continuity".into(),
            "Continuity".into(),
            "Rifle Riven Mod".into(),
        ]);
        assert_eq!(index.position("  continuity "), Some(1));
        assert_eq!(index.search("riv", 5), ["Rifle Riven Mod"]);
        assert_eq!(index.search("", 2).len(), 2);
        assert_eq!(index.search("continuity", 1), ["Continuity"]);
    }

    #[test]
    fn variant_label_formats_rank_and_subtype() {
        let item = Item { max_rank: Some(10), ..item(&[]) };
        assert_eq!(
            variant_label(
                &item,
                Some(&Variant { rank: Some(3), subtype: None })
            ),
            "Rank 3/10"
        );
        assert_eq!(
            variant_label(
                &item,
                Some(&Variant { rank: None, subtype: Some("radiant".into()) })
            ),
            "Radiant"
        );
    }
}
