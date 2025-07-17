use std::mem;

use anyhow::Context as _;
use mongodb::bson::doc;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::application::interaction::Interaction;

use crate::{
    context::Context,
    dbs::mongo::models::channel::ChannelEnum,
    services::{notification::NotificationService, role_message},
    utils::embed,
};
use std::sync::Arc;

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "channel", desc_localizations = "admin_channel_desc")]
pub struct AdminChannelCommand {
    #[command(desc_localizations = "admin_type_arg_desc")]
    pub channel_type: ChannelEnum,
}

fn admin_channel_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Configure the current channel type",
        [("th", "ตั้งค่าประเภทของ text channel ปัจจุบัน")],
    )
}

fn admin_type_arg_desc() -> DescLocalizations {
    DescLocalizations::new("Channel type", [("th", "ชนิดของ text channel")])
}

impl AdminChannelCommand {
    pub async fn run(&self, ctx: Arc<Context>, interaction: Interaction) -> anyhow::Result<()> {
        let guild_id = interaction.guild_id.context("failed to parse guild_id")?;

        let mut interaction = interaction;
        let channel = mem::take(&mut interaction.channel).context("failed to take channel")?;
        let author = interaction.author().context("failed to parse author")?;
        let channel_id = channel.id.get() as i64;
        let channel_name = channel.name.context("failed to parse channel name")?;

        match self.channel_type {
            ChannelEnum::None => {
                ctx.mongo
                    .channels
                    .delete_many(doc! {
                        "channel_id": &channel_id,
                    })
                    .await?;
            }
            _ => {
                ctx.mongo
                    .channels
                    .update_one(
                        doc! {
                            "guild_id": guild_id.get() as i64,
                            "channel_type": self.channel_type.value(),
                        },
                        doc! {
                            "$set": {
                            "guild_id": guild_id.get() as i64,
                            "channel_type": self.channel_type.value(),
                            "channel_id": &channel_id,
                            }
                        },
                    )
                    .upsert(true)
                    .await?;
            }
        };

        if let Some(guild_ref) = ctx.cache.guild(guild_id) {
            let embed = embed::set_channel_embed(
                &guild_ref,
                &channel_name,
                channel_id,
                self.channel_type.value(),
                &author.name,
            )?;
            ctx.http
                .interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;
        }

        if self.channel_type == ChannelEnum::UpdateRole {
            role_message::handler::ensure_message(ctx.clone(), guild_id).await;
        }
        NotificationService::reload_guild(ctx, guild_id.get()).await;
        Ok(())
    }
}
