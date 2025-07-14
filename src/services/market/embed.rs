use twilight_cache_inmemory::{Reference, model::CachedGuild};
use twilight_model::{
    channel::message::{Embed, embed::EmbedField},
    id::{Id, marker::GuildMarker},
};
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use crate::utils::embed::footer_with_icon;

use super::{MarketKind, MarketService, client, session};

const COLOR: u32 = 0xF1C40F;

impl MarketService {
    pub fn not_found_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    ) -> anyhow::Result<Embed> {
        let mut footer = footer_with_icon(guild)?;
        footer.text = guild.name().to_string();
        Ok(EmbedBuilder::new()
            .color(COLOR)
            .title("ไม่พบราคา")
            .description("กรุณาตรวจสอบชื่อ item อีกครั้ง")
            .footer(footer)
            .build())
    }

    pub(super) fn error_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
    ) -> anyhow::Result<Embed> {
        let mut footer = footer_with_icon(guild)?;
        footer.text = guild.name().to_string();
        Ok(EmbedBuilder::new()
            .color(COLOR)
            .title("เกิดข้อผิดพลาด")
            .description("กรุณาลองอีกครั้ง ภายหลัง")
            .footer(footer)
            .build())
    }

    fn build_fields(
        orders: &[session::OrderInfo],
        item: &str,
        kind: &MarketKind,
        rank: Option<u8>,
    ) -> Vec<EmbedField> {
        orders
            .iter()
            .take(5)
            .map(|o| {
                let rank_text = rank.map_or(String::new(), |r| format!(" [ Item Rank : {r} ]"));
                EmbedFieldBuilder::new(
                    format!(
                        "Quantity : {} | Price : {} platinum.{rank_text}",
                        o.quantity, o.platinum
                    ),
                    format!(
                        "```/w {} Hi! I want to {}: \"{}\" for {} platinum. (warframe.market)```",
                        o.ign,
                        kind.action(),
                        item,
                        o.platinum
                    ),
                )
                .build()
            })
            .collect()
    }

    pub(super) fn build_embed(
        guild: &Reference<'_, Id<GuildMarker>, CachedGuild>,
        item: &str,
        url: &str,
        kind: &MarketKind,
        rank: Option<u8>,
        orders: Vec<session::OrderInfo>,
    ) -> anyhow::Result<Embed> {
        let mut footer = footer_with_icon(guild)?;
        footer.text = if let Some(r) = rank {
            format!("{} [ Item Rank : {r} ]", guild.name())
        } else {
            guild.name().to_string()
        };
        let title = if let Some(r) = rank {
            format!("{} {} [Rank {}]", kind.label(), item, r)
        } else {
            format!("{} {}", kind.label(), item)
        };
        let mut builder = EmbedBuilder::new().color(COLOR).title(title).url(format!(
            "{}{}",
            client::ITEM_URL,
            url
        ));
        for field in Self::build_fields(&orders, item, kind, rank) {
            builder = builder.field(field);
        }
        let embed = builder.footer(footer).build();
        Ok(embed)
    }
}
