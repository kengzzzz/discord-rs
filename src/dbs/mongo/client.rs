use mongodb::{
    Client, Collection, IndexModel,
    bson::doc,
    options::{
        ChangeStreamOptions, ClientOptions, Credential, FullDocumentBeforeChangeType,
        FullDocumentType, IndexOptions, Tls, TlsOptions,
    },
};
use std::sync::Arc;

use crate::{
    configs::{app::APP_CONFIG, mongo::MONGO_CONFIGS},
    dbs::mongo::{
        models::{
            ai_prompt::AiPrompt, channel::Channel, message::Message, quarantine::Quarantine,
            role::Role,
        },
        monitor, watchers,
    },
    services::shutdown,
};

pub struct MongoDB {
    pub(crate) client: Client,
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

        for coll in ["channels", "roles", "quarantines", "messages", "ai_prompts"] {
            if let Err(e) = database.create_collection(coll).await {
                tracing::debug!(collection = coll, error = %e, "failed to create collection (might already exist)");
            }
        }

        if !APP_CONFIG.is_atlas() {
            if let Err(e) = database
                .run_command(doc! {
                    "collMod": "channels",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await
            {
                tracing::debug!(collection = "channels", error = %e, "failed to update collection options");
            }
            if let Err(e) = database
                .run_command(doc! {
                    "collMod": "roles",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await
            {
                tracing::debug!(collection = "roles", error = %e, "failed to update collection options");
            }
            if let Err(e) = database
                .run_command(doc! {
                    "collMod": "quarantines",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await
            {
                tracing::debug!(collection = "quarantines", error = %e, "failed to update collection options");
            }
            if let Err(e) = database
                .run_command(doc! {
                    "collMod": "messages",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await
            {
                tracing::debug!(collection = "messages", error = %e, "failed to update collection options");
            }
            if let Err(e) = database
                .run_command(doc! {
                    "collMod": "ai_prompts",
                    "changeStreamPreAndPostImages": { "enabled": true }
                })
                .await
            {
                tracing::debug!(collection = "ai_prompts", error = %e, "failed to update collection options");
            }
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
        if let Err(e) = channels.create_indexes([idx1, idx2]).await {
            tracing::debug!(collection = "channels", error = %e, "failed to create indexes");
        }

        let idx1 = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "role_type": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let idx2: IndexModel = IndexModel::builder()
            .keys(doc! { "role_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        if let Err(e) = roles.create_indexes([idx1, idx2]).await {
            tracing::debug!(collection = "roles", error = %e, "failed to create indexes");
        }

        let idx = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "user_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        if let Err(e) = quarantines.create_index(idx).await {
            tracing::debug!(collection = "quarantines", error = %e, "failed to create index");
        }

        let idx = IndexModel::builder()
            .keys(doc! { "guild_id": 1, "message_type": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        if let Err(e) = messages.create_index(idx).await {
            tracing::debug!(collection = "messages", error = %e, "failed to create index");
        }

        let idx = IndexModel::builder()
            .keys(doc! { "user_id": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        if let Err(e) = ai_prompts.create_index(idx).await {
            tracing::debug!(collection = "ai_prompts", error = %e, "failed to create index");
        }

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
            .full_document_before_change(Some(FullDocumentBeforeChangeType::WhenAvailable))
            .build();

        let token = shutdown::get_token();
        watchers::spawn_channel_watcher(repo.channels.clone(), options.clone(), token.clone())
            .await?;
        watchers::spawn_role_watcher(repo.roles.clone(), options.clone(), token.clone()).await?;
        watchers::spawn_quarantine_watcher(
            repo.quarantines.clone(),
            options.clone(),
            token.clone(),
        )
        .await?;
        watchers::spawn_message_watcher(repo.messages.clone(), options.clone(), token.clone())
            .await?;
        watchers::spawn_ai_prompt_watcher(repo.ai_prompts.clone(), options, token.clone()).await?;
        monitor::spawn_monitor(repo.clone());

        Ok(repo)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}
