use std::sync::Arc;

use twilight_interactions::command::{
    CommandModel, CommandOption, CreateCommand, CreateOption, DescLocalizations,
};
use twilight_model::application::interaction::Interaction;

use crate::{
    app::App,
    discord::reply,
    warframe::{builds, circuit, market, status},
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(
    name = "warframe",
    desc_localizations = "warframe_desc",
    contexts = "guild",
    integration_types = "guild_install"
)]
pub enum WarframeCommand {
    #[command(name = "market")]
    Market(MarketCommand),
    #[command(name = "build")]
    Build(BuildCommand),
    #[command(name = "circuit")]
    Circuit(CircuitCommand),
    #[command(name = "status")]
    Status(StatusCommand),
}

#[derive(CreateOption, CommandOption, Clone, Copy, Debug)]
pub enum MarketSide {
    #[option(name = "Buy (show sellers)", value = "buy")]
    Buy,
    #[option(name = "Sell (show buyers)", value = "sell")]
    Sell,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "market", desc_localizations = "market_desc")]
pub struct MarketCommand {
    #[command(
        desc_localizations = "item_desc",
        autocomplete = true,
        max_length = 100
    )]
    pub item: String,
    #[command(desc_localizations = "side_desc")]
    pub side: Option<MarketSide>,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "build", desc_localizations = "build_desc")]
pub struct BuildCommand {
    #[command(
        desc_localizations = "item_desc",
        autocomplete = true,
        max_length = 100
    )]
    pub item: String,
}

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "circuit", desc_localizations = "circuit_desc")]
pub struct CircuitCommand;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "status", desc_localizations = "status_desc")]
pub struct StatusCommand;

fn warframe_desc() -> DescLocalizations {
    DescLocalizations::new("Warframe tools", [("th", "เครื่องมือ Warframe")])
}
fn market_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Check live prices on warframe.market",
        [("th", "เช็คราคาบน warframe.market")],
    )
}
fn item_desc() -> DescLocalizations {
    DescLocalizations::new("Item name", [("th", "ชื่อไอเทม")])
}
fn side_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Buy or sell (default: buy)",
        [("th", "ซื้อหรือขาย (ค่าเริ่มต้น: ซื้อ)")],
    )
}
fn build_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Find builds on overframe.gg",
        [("th", "ค้นหา build จาก overframe.gg")],
    )
}
fn circuit_desc() -> DescLocalizations {
    DescLocalizations::new(
        "This week's Circuit and upcoming Incarnons",
        [("th", "Circuit สัปดาห์นี้และ Incarnon สัปดาห์ถัดไป")],
    )
}
fn status_desc() -> DescLocalizations {
    DescLocalizations::new(
        "World cycles and weekly resets",
        [("th", "สถานะโลกและรายการรีเซ็ตรายสัปดาห์")],
    )
}

pub async fn run(
    app: &Arc<App>,
    interaction: &Interaction,
    command: WarframeCommand,
) -> anyhow::Result<()> {
    match command {
        WarframeCommand::Market(cmd) => {
            let kind = match cmd.side.unwrap_or(MarketSide::Buy) {
                MarketSide::Buy => market::Kind::Buy,
                MarketSide::Sell => market::Kind::Sell,
            };
            market::command(app, interaction, &cmd.item, kind).await
        }
        WarframeCommand::Build(cmd) => builds::command(app, interaction, &cmd.item).await,
        WarframeCommand::Circuit(_) => {
            let snapshot = app.warframe.snapshot();
            reply::respond(
                app,
                interaction,
                circuit::card(
                    snapshot
                        .as_ref()
                        .and_then(|s| s.circuit.as_ref()),
                ),
                true,
            )
            .await
        }
        WarframeCommand::Status(_) => {
            let snapshot = app.warframe.snapshot();
            let components = status::render(
                app,
                snapshot.as_deref(),
                jiff::Timestamp::now().as_millisecond(),
            );
            reply::respond(app, interaction, components, true).await
        }
    }
}
