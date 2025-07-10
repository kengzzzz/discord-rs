use anyhow::Context;
use mongodb::bson::doc;
use twilight_interactions::command::{CommandModel, CreateCommand, DescLocalizations};
use twilight_model::{
    application::interaction::Interaction,
    id::{Id, marker::RoleMarker},
};

use crate::{
    configs::discord::{CACHE, HTTP},
    dbs::mongo::{mongodb::MongoDB, role::RoleEnum},
    services::{notification::NotificationService, role_message::RoleMessageService},
    utils::embed,
};

#[derive(CommandModel, CreateCommand, Debug)]
#[command(name = "role", desc_localizations = "admin_role_desc")]
pub struct AdminRoleCommand {
    #[command(desc_localizations = "role_type_arg_desc")]
    pub role_type: RoleEnum,

    #[command(desc_localizations = "role_name_arg_desc", autocomplete = true)]
    pub role_name: String,

    #[command(desc_localizations = "role_assign_arg_desc")]
    pub self_assignable: Option<bool>,
}

fn admin_role_desc() -> DescLocalizations {
    DescLocalizations::new("Configure role settings", [("th", "ตั้งค่า role")])
}

fn role_type_arg_desc() -> DescLocalizations {
    DescLocalizations::new("Select the role type", [("th", "เลือกชนิดของ role")])
}

fn role_name_arg_desc() -> DescLocalizations {
    DescLocalizations::new("Select the role name", [("th", "เลือกชื่อของ role")])
}

fn role_assign_arg_desc() -> DescLocalizations {
    DescLocalizations::new(
        "Whether the role is self‑assignable",
        [("th", "กำหนดได้ว่า role นี้ผู้ใช้สามารถกดรับเองได้หรือไม่")],
    )
}

impl AdminRoleCommand {
    pub async fn run(&self, interaction: Interaction) -> anyhow::Result<()> {
        let guild_id = interaction.guild_id.context("failed to parse guild_id")?;
        let role_type = self.role_type.value();
        let role_id: Id<RoleMarker> = self.role_name.parse()?;
        let self_assignable = self.self_assignable.unwrap_or_default();

        let author = interaction.author().context("failed to parse author")?;

        let db = MongoDB::get();

        match self.role_type {
            RoleEnum::None => {
                db.roles
                    .delete_many(doc! {
                        "role_id": role_id.get() as i64,
                    })
                    .await?;
            }
            _ => {
                db.roles
                    .update_one(
                        doc! {
                            "guild_id": guild_id.get() as i64,
                            "role_type": role_type,
                        },
                        doc! {
                            "$set": {
                            "guild_id": guild_id.get() as i64,
                            "role_type": role_type,
                            "role_id": role_id.get() as i64,
                            "self_assignable": self_assignable,
                            }
                        },
                    )
                    .upsert(true)
                    .await?;
            }
        };

        if let Some(guild_ref) = CACHE.guild(guild_id) {
            let embed = embed::set_role_embed(
                &guild_ref,
                &self.role_name,
                role_id.get(),
                self.role_type.value(),
                &author.name,
            )?;
            HTTP.interaction(interaction.application_id)
                .update_response(&interaction.token)
                .embeds(Some(&[embed]))
                .await?;
        }

        RoleMessageService::ensure_message(guild_id).await;
        NotificationService::reload_guild(guild_id.get()).await;

        Ok(())
    }
}
