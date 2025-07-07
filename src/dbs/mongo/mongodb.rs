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
use tokio::sync::OnceCell;
use tokio::time::{self, Duration};

use crate::{
    configs::{app::APP_CONFIG, mongo::MONGO_CONFIGS},
    dbs::mongo::{
        channel::Channel, quarantine::Quarantine, role::Role, role_message::RoleMessage,
        status_message::StatusMessage, watcher::spawn_watcher,
    },
    services::{
        channel::ChannelService, health::HealthService, role::RoleService,
        role_message::RoleMessageService, spam::SpamService, status_message::StatusMessageService,
    },
};

pub struct MongoDB {
    client: Client,
    pub channels: Collection<Channel>,
    pub roles: Collection<Role>,
    pub quarantines: Collection<Quarantine>,
    pub role_messages: Collection<RoleMessage>,
    pub status_messages: Collection<StatusMessage>,
}

static MONGO_DB: OnceCell<Arc<MongoDB>> = OnceCell::const_new();

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
        let _ = database.create_collection("role_messages").await;
        let _ = database.create_collection("status_messages").await;

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
                    "collMod": "role_messages",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
            let _ = database
                .run_command(doc! {
                    "collMod": "status_messages",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await;
        }

        let channels = database.collection::<Channel>("channels");
        let roles = database.collection::<Role>("roles");
        let quarantines = database.collection::<Quarantine>("quarantines");
        let role_messages = database.collection::<RoleMessage>("role_messages");
        let status_messages = database.collection::<StatusMessage>("status_messages");

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
            .keys(doc! { "guild_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let _ = role_messages.create_index(idx).await;

        let idx = IndexModel::builder()
            .keys(doc! { "guild_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let _ = status_messages.create_index(idx).await;

        let repo = Arc::new(Self {
            client,
            channels,
            roles,
            quarantines,
            role_messages,
            status_messages,
        });

        let options = ChangeStreamOptions::builder()
            .full_document(Some(FullDocumentType::UpdateLookup))
            .full_document_before_change(Some(FullDocumentBeforeChangeType::Required))
            .build();

        spawn_watcher(repo.channels.clone(), options.clone(), |evt| async move {
            match evt.operation_type {
                OperationType::Insert
                | OperationType::Update
                | OperationType::Replace
                | OperationType::Delete => {
                    if let Some(doc) = evt.full_document {
                        ChannelService::purge_cache(doc.channel_id).await;
                        ChannelService::purge_cache_by_type(doc.guild_id, &doc.channel_type).await;
                        ChannelService::purge_list_cache(&doc.channel_type).await;
                    }
                    if let Some(doc) = evt.full_document_before_change {
                        ChannelService::purge_cache(doc.channel_id).await;
                        ChannelService::purge_cache_by_type(doc.guild_id, &doc.channel_type).await;
                        ChannelService::purge_list_cache(&doc.channel_type).await;
                    }
                }
                _ => {}
            }
        })
        .await?;
        spawn_watcher(repo.roles.clone(), options.clone(), |evt| async move {
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
        })
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
        )
        .await?;
        spawn_watcher(
            repo.role_messages.clone(),
            options.clone(),
            |evt| async move {
                match evt.operation_type {
                    OperationType::Insert
                    | OperationType::Update
                    | OperationType::Replace
                    | OperationType::Delete => {
                        if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                            RoleMessageService::purge_cache(doc.guild_id).await;
                        }
                    }
                    _ => {}
                }
            },
        )
        .await?;
        spawn_watcher(repo.status_messages.clone(), options, |evt| async move {
            match evt.operation_type {
                OperationType::Insert
                | OperationType::Update
                | OperationType::Replace
                | OperationType::Delete => {
                    if let Some(doc) = evt.full_document.or(evt.full_document_before_change) {
                        StatusMessageService::purge_cache(doc.guild_id).await;
                    }
                }
                _ => {}
            }
        })
        .await?;

        MONGO_DB
            .set(repo.clone())
            .map_err(|_| anyhow::anyhow!("MongoDB already initialized"))?;

        HealthService::set_mongo(true);

        let weak = Arc::downgrade(&repo);
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
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

    pub fn get() -> Arc<Self> {
        Arc::clone(MONGO_DB.get().expect("MongoDB not initialized."))
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn is_initialized() -> bool {
        MONGO_DB.get().is_some()
    }
}
