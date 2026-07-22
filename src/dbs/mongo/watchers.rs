use mongodb::{
    Collection,
    change_stream::event::{ChangeStreamEvent, OperationType},
    options::ChangeStreamOptions,
};
use tokio_util::sync::CancellationToken;
use twilight_model::id::Id;

use crate::services::{
    ai::AiService, channel::ChannelService, guild_settings::GuildSettingsService,
    role::RoleService, role_message, spam, status_message::StatusMessageService,
};
use crate::{
    configs::CACHE_PREFIX,
    dbs::{
        mongo::{
            models::{
                ai_prompt::AiPrompt,
                channel::Channel,
                guild_settings::GuildSettings,
                message::{Message, MessageEnum},
                quarantine::Quarantine,
                role::Role,
            },
            watcher::spawn_watcher,
        },
        redis::redis_delete_prefixes,
    },
};

use deadpool_redis::Pool;

enum Invalidation<T> {
    Documents(Vec<T>),
    Sweep(OperationType),
    Ignore,
}

fn invalidation_for<T>(evt: ChangeStreamEvent<T>) -> Invalidation<T> {
    match evt.operation_type {
        OperationType::Insert => evt
            .full_document
            .map(|document| Invalidation::Documents(vec![document]))
            .unwrap_or(Invalidation::Sweep(OperationType::Insert)),
        operation @ (OperationType::Update | OperationType::Replace) => {
            match (evt.full_document, evt.full_document_before_change) {
                (Some(document), Some(previous)) => {
                    Invalidation::Documents(vec![document, previous])
                }
                _ => Invalidation::Sweep(operation),
            }
        }
        OperationType::Delete => evt
            .full_document_before_change
            .map(|document| Invalidation::Documents(vec![document]))
            .unwrap_or(Invalidation::Sweep(OperationType::Delete)),
        _ => Invalidation::Ignore,
    }
}

async fn sweep_cache(pool: &Pool, collection: &str, operation: OperationType, prefixes: &[String]) {
    let deleted = redis_delete_prefixes(pool, prefixes).await;
    tracing::warn!(
        collection,
        ?operation,
        deleted,
        "change event missing required document image; purged collection cache prefixes"
    );
}

async fn handle_channel_event(pool: &Pool, evt: ChangeStreamEvent<Channel>) {
    match invalidation_for(evt) {
        Invalidation::Documents(documents) => {
            for document in documents {
                ChannelService::purge_cache(pool, document.channel_id).await;
                ChannelService::purge_cache_by_type(
                    pool,
                    document.guild_id,
                    &document.channel_type,
                )
                .await;
                ChannelService::purge_list_cache(pool, &document.channel_type).await;
            }
        }
        Invalidation::Sweep(operation) => {
            sweep_cache(
                pool,
                "channels",
                operation,
                &[
                    format!("{CACHE_PREFIX}:channel:"),
                    format!("{CACHE_PREFIX}:channel-type:"),
                    format!("{CACHE_PREFIX}:channels-by-type:"),
                ],
            )
            .await;
        }
        Invalidation::Ignore => {}
    }
}

async fn handle_role_event(pool: &Pool, evt: ChangeStreamEvent<Role>) {
    match invalidation_for(evt) {
        Invalidation::Documents(documents) => {
            for document in documents {
                RoleService::purge_cache(pool, document.role_id).await;
                RoleService::purge_cache_by_type(pool, document.guild_id, &document.role_type)
                    .await;
            }
        }
        Invalidation::Sweep(operation) => {
            sweep_cache(
                pool,
                "roles",
                operation,
                &[format!("{CACHE_PREFIX}:role:"), format!("{CACHE_PREFIX}:role-type:")],
            )
            .await;
        }
        Invalidation::Ignore => {}
    }
}

async fn handle_quarantine_event(pool: &Pool, evt: ChangeStreamEvent<Quarantine>) {
    match invalidation_for(evt) {
        Invalidation::Documents(documents) => {
            for document in documents {
                spam::quarantine::purge_cache(pool, document.guild_id, document.user_id).await;
            }
        }
        Invalidation::Sweep(operation) => {
            sweep_cache(
                pool,
                "quarantines",
                operation,
                &[
                    "spam:quarantine:".to_owned(),
                    "spam:log:".to_owned(),
                    "spam:campaign:".to_owned(),
                ],
            )
            .await;
        }
        Invalidation::Ignore => {}
    }
}

async fn handle_message_event(pool: &Pool, evt: ChangeStreamEvent<Message>) {
    match invalidation_for(evt) {
        Invalidation::Documents(documents) => {
            for document in documents {
                match document.message_type {
                    MessageEnum::Role => {
                        role_message::storage::purge_cache(pool, document.guild_id).await;
                    }
                    MessageEnum::Status => {
                        StatusMessageService::purge_cache(pool, document.guild_id).await;
                    }
                }
            }
        }
        Invalidation::Sweep(operation) => {
            sweep_cache(
                pool,
                "messages",
                operation,
                &[
                    format!("{CACHE_PREFIX}:role-message:"),
                    format!("{CACHE_PREFIX}:status-message:"),
                ],
            )
            .await;
        }
        Invalidation::Ignore => {}
    }
}

async fn handle_ai_prompt_event(pool: &Pool, evt: ChangeStreamEvent<AiPrompt>) {
    match invalidation_for(evt) {
        Invalidation::Documents(documents) => {
            for document in documents {
                AiService::purge_prompt_cache(pool, document.user_id).await;
                AiService::clear_history(pool, Id::new(document.user_id)).await;
            }
        }
        Invalidation::Sweep(operation) => {
            sweep_cache(
                pool,
                "ai_prompts",
                operation,
                &[format!("{CACHE_PREFIX}:ai:prompt:"), format!("{CACHE_PREFIX}:ai:history:")],
            )
            .await;
        }
        Invalidation::Ignore => {}
    }
}

async fn handle_guild_settings_event(pool: &Pool, evt: ChangeStreamEvent<GuildSettings>) {
    match invalidation_for(evt) {
        Invalidation::Documents(documents) => {
            for document in documents {
                GuildSettingsService::purge_cache(pool, document.guild_id).await;
            }
        }
        Invalidation::Sweep(operation) => {
            sweep_cache(
                pool,
                "guild_settings",
                operation,
                &[format!("{CACHE_PREFIX}:guild-settings:")],
            )
            .await;
        }
        Invalidation::Ignore => {}
    }
}

pub async fn spawn_channel_watcher(
    coll: Collection<Channel>,
    options: ChangeStreamOptions,
    pool: Pool,
    token: CancellationToken,
) -> anyhow::Result<()> {
    spawn_watcher(
        coll,
        options,
        pool.clone(),
        move |evt| {
            let pool = pool.clone();
            async move { handle_channel_event(&pool, evt).await }
        },
        token,
    )
    .await
}

pub async fn spawn_role_watcher(
    coll: Collection<Role>,
    options: ChangeStreamOptions,
    pool: Pool,
    token: CancellationToken,
) -> anyhow::Result<()> {
    spawn_watcher(
        coll,
        options,
        pool.clone(),
        move |evt| {
            let pool = pool.clone();
            async move { handle_role_event(&pool, evt).await }
        },
        token,
    )
    .await
}

pub async fn spawn_quarantine_watcher(
    coll: Collection<Quarantine>,
    options: ChangeStreamOptions,
    pool: Pool,
    token: CancellationToken,
) -> anyhow::Result<()> {
    spawn_watcher(
        coll,
        options,
        pool.clone(),
        move |evt| {
            let pool = pool.clone();
            async move { handle_quarantine_event(&pool, evt).await }
        },
        token,
    )
    .await
}

pub async fn spawn_message_watcher(
    coll: Collection<Message>,
    options: ChangeStreamOptions,
    pool: Pool,
    token: CancellationToken,
) -> anyhow::Result<()> {
    spawn_watcher(
        coll,
        options,
        pool.clone(),
        move |evt| {
            let pool = pool.clone();
            async move { handle_message_event(&pool, evt).await }
        },
        token,
    )
    .await
}

pub async fn spawn_ai_prompt_watcher(
    coll: Collection<AiPrompt>,
    options: ChangeStreamOptions,
    pool: Pool,
    token: CancellationToken,
) -> anyhow::Result<()> {
    spawn_watcher(
        coll,
        options,
        pool.clone(),
        move |evt| {
            let pool = pool.clone();
            async move { handle_ai_prompt_event(&pool, evt).await }
        },
        token,
    )
    .await
}

pub async fn spawn_guild_settings_watcher(
    coll: Collection<GuildSettings>,
    options: ChangeStreamOptions,
    pool: Pool,
    token: CancellationToken,
) -> anyhow::Result<()> {
    spawn_watcher(
        coll,
        options,
        pool.clone(),
        move |evt| {
            let pool = pool.clone();
            async move { handle_guild_settings_event(&pool, evt).await }
        },
        token,
    )
    .await
}

#[cfg(test)]
#[path = "tests/watchers.rs"]
mod tests;
