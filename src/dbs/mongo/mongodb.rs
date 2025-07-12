use mongodb::{
    Client, Collection, IndexModel,
    bson::doc,
    change_stream::event::OperationType,
    options::{
        ChangeStreamOptions, ClientOptions, Credential, FullDocumentBeforeChangeType,
        FullDocumentType, IndexOptions, Tls, TlsOptions,
    },
};
use std::sync::Arc;
use tokio::time::{self, Duration};
use twilight_model::id::Id;

use crate::{
    configs::{app::APP_CONFIG, mongo::MONGO_CONFIGS},
    dbs::mongo::{
        ai_prompt::AiPrompt,
        channel::Channel,
        message::{Message, MessageEnum},
        quarantine::Quarantine,
        role::Role,
        watcher::spawn_watcher,
    },
    services::{
        ai::AiService, channel::ChannelService, health::HealthService, role::RoleService,
        role_message::RoleMessageService, shutdown, spam::SpamService,
        status_message::StatusMessageService,
    },
};

pub struct MongoDB {
    client: Client,
    pub channels: Collection<Channel>,
    pub roles: Collection<Role>,
    pub quarantines: Collection<Quarantine>,
    pub messages: Collection<Message>,
    pub ai_prompts: Collection<AiPrompt>,
}

impl MongoDB {
    pub async fn init() -> anyhow::Result<Arc<Self>> {
        let mut opts = ClientOptions::parse(&MONGO_CONFIGS.uri).await?;
        opts.credential = Some(
            Credential::builder()
                .username(MONGO_CONFIGS.username.clone())
                .password(MONGO_CONFIGS.password.clone())
                .source(MONGO_CONFIGS.auth_source.clone())
                .build(),
        );
        if MONGO_CONFIGS.ssl {
            let mut tls_opts = TlsOptions::default();
            if let Some(ref ca) = MONGO_CONFIGS.ca_file_path {
                tls_opts.ca_file_path = Some(ca.into());
            }
            if let Some(ref cert) = MONGO_CONFIGS.cert_key_file_path {
                tls_opts.cert_key_file_path = Some(cert.into());
            }
            if let Some(v) = MONGO_CONFIGS.allow_invalid_certificates {
                tls_opts.allow_invalid_certificates = Some(v);
            }
            opts.tls = Some(Tls::Enabled(tls_opts));
        } else {
            opts.tls = Some(Tls::Disabled);
        }

        let client = Client::with_options(opts)?;
        let database = client.database(&MONGO_CONFIGS.database);

        let _ = database.create_collection("channels").await;
        let _ = database.create_collection("roles").await;
        let _ = database.create_collection("quarantines").await;
        let _ = database.create_collection("messages").await;
        let _ = database.create_collection("ai_prompts").await;

        if !APP_CONFIG.is_atlas() {
            let _ = database
                .run_command(doc! {
                    "collMod": "channels",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
            let _ = database
                .run_command(doc! {
                    "collMod": "roles",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
            let _ = database
                .run_command(doc! {
                    "collMod": "quarantines",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
            let _ = database
                .run_command(doc! {
                    "collMod": "messages",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
            let _ = database
                .run_command(doc! {
                    "collMod": "ai_prompts",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
        }

        let channels = database.collection::<Channel>("channels");
        let roles = database.collection::<Role>("roles");
        let quarantines = database.collection::<Quarantine>("quarantines");
        let messages = database.collection::<Message>("messages");
        let ai_prompts = database.collection::<AiPrompt>("ai_prompts");

        let idx1 = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "channel_type": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let idx2 = IndexModel::builder().keys(doc! { "channel_id": 1 }).build();
        let _ = channels.create_indexes([idx1, idx2]).await;

        let idx1 = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "role_type": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let idx2: IndexModel = IndexModel::builder()
            .keys(doc! { "role_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let _ = roles.create_indexes([idx1, idx2]).await;

        let idx = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "user_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let _ = quarantines.create_index(idx).await;

        let idx = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "message_type": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let _ = messages.create_index(idx).await;

        let idx = IndexModel::builder()
            .keys(doc! { "user_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let _ = ai_prompts.create_index(idx).await;

        let repo = Arc::new(Self {
            client,
            channels,
            roles,
            quarantines,
            messages,
            ai_prompts,
        });

        let options = ChangeStreamOptions::builder()
            .full_document(Some(FullDocumentType::UpdateLookup))
            .full_document_before_change(Some(FullDocumentBeforeChangeType::Required))
            .build();

        let token = shutdown::get_token();
        spawn_watcher(
            repo.channels.clone(),
            options.clone(),
            |evt| async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document {
                            ChannelService::purge_cache(doc.channel_id).await;
                            ChannelService::purge_cache_by_type(doc.guild_id, &doc.channel_type)
                                .await;
                            ChannelService::purge_list_cache(&doc.channel_type).await;
                        }
                        if let Some(doc) = evt.full_document_before_change {
                            ChannelService::purge_cache(doc.channel_id).await;
                            ChannelService::purge_cache_by_type(doc.guild_id, &doc.channel_type)
                                .await;
                            ChannelService::purge_list_cache(&doc.channel_type).await;
                        }
                    }
                    _ => {}
                }
            },
            token.clone(),
        )
        .await?;
        spawn_watcher(
            repo.roles.clone(),
            options.clone(),
            |evt| async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document {
                            RoleService::purge_cache(doc.role_id).await;
                            RoleService::purge_cache_by_type(doc.guild_id, &doc.role_type).await;
                        }
                        if let Some(doc) = evt.full_document_before_change {
                            RoleService::purge_cache(doc.role_id).await;
                            RoleService::purge_cache_by_type(doc.guild_id, &doc.role_type).await;
                        }
                    }
                    _ => {}
                }
            },
            token.clone(),
        )
        .await?;
        spawn_watcher(
            repo.quarantines.clone(),
            options.clone(),
            |evt| async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            SpamService::purge_cache(doc.guild_id, doc.user_id).await;
                        }
                    }
                    _ => {}
                }
            },
            token.clone(),
        )
        .await?;
        spawn_watcher(
            repo.messages.clone(),
            options.clone(),
            |evt| async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            match doc.message_type {
                                MessageEnum::Role => {
                                    RoleMessageService::purge_cache(doc.guild_id).await
                                }
                                MessageEnum::Status => {
                                    StatusMessageService::purge_cache(doc.guild_id).await
                                }
                            };
                        }
                    }
                    _ => {}
                }
            },
            token.clone(),
        )
        .await?;
        spawn_watcher(
            repo.ai_prompts.clone(),
            options,
            |evt| async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            AiService::purge_prompt_cache(doc.user_id).await;
                            AiService::clear_history(Id::new(doc.user_id)).await;
                        }
                    }
                    _ => {}
                }
            },
            token.clone(),
        )
        .await?;

        let weak = Arc::downgrade(&repo);
        tokio::spawn(async move {
            let token = shutdown::get_token();
            let mut interval = time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = interval.tick() => {}
                }
                if let Some(db) = weak.upgrade() {
                    let ok = db
                        .client()
                        .database("admin")
                        .run_command(doc! { "ping": 1 })
                        .await
                        .is_ok();
                    HealthService::set_mongo(ok);
                } else {
                    break;
                }
            }
        });

        Ok(repo)
    }

    #[cfg(test)]
    pub async fn empty() -> Arc<Self> {
        let client = Client::with_uri_str("mongodb://localhost:27017")
            .await
            .unwrap();
        let db = client.database("test");
        Arc::new(Self {
            client,
            channels: db.collection("channels"),
            roles: db.collection("roles"),
            quarantines: db.collection("quarantines"),
            messages: db.collection("messages"),
            ai_prompts: db.collection("ai_prompts"),
        })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}
