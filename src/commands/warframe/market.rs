use anyhow::Context as _;
use twilight_interactions::command::{
    CommandModel, CommandOption, CreateCommand, CreateOption, DescLocalizations,
};
use twilight_model::application::interaction::Interaction;

use crate::{
    context::Context,
    services::market::{MarketKind, MarketService},
};
use std::sync::Arc;

#[derive(CreateOption, CommandOption, Clone, Copy, Debug)]
#[repr(u8)]
pub enum MarketType {
    #[option(name = "Buy", value = "buy")]
    Buy,
    #[option(name = "Sell", value = "sell")]
    Sell,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "market", desc_localizations = "market_desc")]
pub struct WarframeMarketCommand {
    #[command(desc_localizations = "market_item_desc", autocomplete = true)]
    pub item: String,
    #[command(desc_localizations = "market_type_desc")]
    pub kind: MarketType,
}

fn market_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Check prices on warframe.market",
        [("th", "เช็คราคา warframe.market")],
    )
}

fn market_item_desc() -> DescLocalizations {
    DescLocalizations::new("Item name", [("th", "ชื่อไอเทม")])
}

fn market_type_desc() -> DescLocalizations {
    DescLocalizations::new("Buy or sell", [("th", "ต้องการซื้อหรือขาย")])
}

impl From<MarketType> for MarketKind {
    fn from(value: MarketType) -> Self {
        match value {
            MarketType::Buy => MarketKind::Buy,
            MarketType::Sell => MarketKind::Sell,
        }
    }
}

impl WarframeMarketCommand {
    pub async fn run(&self, ctx: Arc<Context>, interaction: Interaction) -> anyhow::Result<()> {
        let guild_id = interaction.guild_id.context("parse guild_id failed")?;
        if let Some(guild_ref) = ctx.cache.guild(guild_id) {
            if let Some(session) =
                MarketService::create_session(ctx.clone(), &self.item, self.kind.into()).await?
            {
                let embed = MarketService::embed_for_session(&guild_ref, &session)?;
                let components = MarketService::components(&session);
                let message = ctx
                    .http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .components(Some(&components))
                    .await?
                    .model()
                    .await?;
                MarketService::insert_session(message.id, session);
            } else {
                let embed = MarketService::not_found_embed(&guild_ref)?;
                ctx.http
                    .interaction(interaction.application_id)
                    .update_response(&interaction.token)
                    .embeds(Some(&[embed]))
                    .await?;
            }
        }
        Ok(())
    }
}
