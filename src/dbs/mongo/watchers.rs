use mongodb::{Collection, change_stream::event::OperationType, options::ChangeStreamOptions};
use tokio_util::sync::CancellationToken;
use twilight_model::id::Id;

use crate::dbs::mongo::{
    models::{
        ai_prompt::AiPrompt,
        channel::Channel,
        message::{Message, MessageEnum},
        quarantine::Quarantine,
        role::Role,
    },
    watcher::spawn_watcher,
};
use crate::services::{
    ai::AiService, channel::ChannelService, role::RoleService, role_message, spam,
    status_message::StatusMessageService,
};

use deadpool_redis::Pool;

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
            async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document {
                            ChannelService::purge_cache(&pool, doc.channel_id).await;
                            ChannelService::purge_cache_by_type(
                                &pool,
                                doc.guild_id,
                                &doc.channel_type,
                            )
                            .await;
                            ChannelService::purge_list_cache(&pool, &doc.channel_type).await;
                        }
                        if let Some(doc) = evt.full_document_before_change {
                            ChannelService::purge_cache(&pool, doc.channel_id).await;
                            ChannelService::purge_cache_by_type(
                                &pool,
                                doc.guild_id,
                                &doc.channel_type,
                            )
                            .await;
                            ChannelService::purge_list_cache(&pool, &doc.channel_type).await;
                        }
                    }
                    _ => {}
                }
            }
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
            async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document {
                            RoleService::purge_cache(&pool, doc.role_id).await;
                            RoleService::purge_cache_by_type(&pool, doc.guild_id, &doc.role_type)
                                .await;
                        }
                        if let Some(doc) = evt.full_document_before_change {
                            RoleService::purge_cache(&pool, doc.role_id).await;
                            RoleService::purge_cache_by_type(&pool, doc.guild_id, &doc.role_type)
                                .await;
                        }
                    }
                    _ => {}
                }
            }
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
            async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            spam::quarantine::purge_cache(&pool, doc.guild_id, doc.user_id).await;
                        }
                    }
                    _ => {}
                }
            }
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
            async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            match doc.message_type {
                                MessageEnum::Role => {
                                    role_message::storage::purge_cache(&pool, doc.guild_id).await;
                                }
                                MessageEnum::Status => {
                                    StatusMessageService::purge_cache(&pool, doc.guild_id).await;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
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
            async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            AiService::purge_prompt_cache(&pool, doc.user_id).await;
                            AiService::clear_history(&pool, Id::new(doc.user_id)).await;
                        }
                    }
                    _ => {}
                }
            }
        },
        token,
    )
    .await
}
