use std::{collections::BTreeMap, path::PathBuf};

use anyhow::Context as _;
use futures::TryStreamExt as _;
use mongodb::{
    Client,
    bson::{Bson, Document},
    options::{ClientOptions, Credential, Tls, TlsOptions},
};
use twilight_model::id::Id;

use discord_bot::{
    config,
    db::{
        self, Db,
        messages::{BotMessage, BotMessageKind},
        quarantine::{Reason, Record, State},
    },
    guild::{ChannelKind, RoleBinding, RoleKind},
    warframe::cycles,
};

fn env(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn id(doc: &Document, key: &str) -> Option<u64> {
    let value = match doc.get(key)? {
        Bson::Int64(v) => *v as u64,
        Bson::Int32(v) => *v as u64,
        Bson::Double(v) => *v as u64,
        Bson::String(v) => v.parse().ok()?,
        _ => return None,
    };
    (value > 0).then_some(value)
}

fn text<'a>(doc: &'a Document, key: &str) -> Option<&'a str> {
    doc.get_str(key).ok()
}

async fn connect() -> anyhow::Result<mongodb::Database> {
    let uri = env("MONGO_URI", "mongodb://localhost:27017");
    let mut opts = ClientOptions::parse(&uri)
        .await
        .context("parse MONGO_URI")?;
    let password = config::secret("MONGO_PASSWORD")?
        .context("MONGO_PASSWORD or MONGO_PASSWORD_FILE is required")?;
    opts.credential = Some(
        Credential::builder()
            .username(env("MONGO_USERNAME", ""))
            .password(password)
            .source(env("MONGO_AUTH_SOURCE", "admin"))
            .build(),
    );
    if env("MONGO_SSL", "false").eq_ignore_ascii_case("true") {
        let mut tls = TlsOptions::default();
        if let Ok(ca) = std::env::var("MONGO_TLS_CA_FILE")
            && !ca.is_empty()
        {
            tls.ca_file_path = Some(ca.into());
        }
        if let Ok(cert) = std::env::var("MONGO_TLS_CERT_KEY_FILE")
            && !cert.is_empty()
        {
            tls.cert_key_file_path = Some(cert.into());
        }
        if let Ok(insecure) = std::env::var("MONGO_TLS_INSECURE")
            && !insecure.is_empty()
        {
            tls.allow_invalid_certificates = Some(insecure.eq_ignore_ascii_case("true"));
        }
        opts.tls = Some(Tls::Enabled(tls));
    } else {
        opts.tls = Some(Tls::Disabled);
    }
    opts.app_name = Some("discord-rs-migrate".into());
    let client = Client::with_options(opts)?;
    Ok(client.database(&env("MONGO_DATABASE", "develop")))
}

async fn all(db: &mongodb::Database, name: &str) -> anyhow::Result<Vec<Document>> {
    Ok(db
        .collection::<Document>(name)
        .find(Document::new())
        .await?
        .try_collect()
        .await?)
}

#[derive(Default)]
struct Report {
    counts: BTreeMap<&'static str, (usize, usize)>,
}

impl Report {
    fn seen(&mut self, key: &'static str, imported: bool) {
        let entry = self.counts.entry(key).or_default();
        entry.0 += 1;
        if imported {
            entry.1 += 1;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("failed to install rustls crypto provider"))?;
    let target = PathBuf::from(env("DATABASE_PATH", "/data/bot.sqlite3"));
    let sqlite = Db::open(&target)?;
    let has_data = sqlite
        .call(|c| {
            c.query_row(
                "SELECT EXISTS(SELECT 1 FROM guild_channel) OR EXISTS(SELECT 1 FROM guild_role)
                 OR EXISTS(SELECT 1 FROM guild_settings) OR EXISTS(SELECT 1 FROM quarantine)",
                [],
                |row| row.get::<_, bool>(0),
            )
        })
        .await?;
    // Importing again would undo releases and panel changes made since.
    if has_data && !std::env::args().any(|a| a == "--force") {
        anyhow::bail!(
            "{} already has settings or quarantines; pass --force to import over them",
            target.display()
        );
    }
    let mongo = connect().await?;
    let mut report = Report::default();
    let mut notification_guilds = Vec::new();

    for doc in all(&mongo, "channels").await? {
        let parsed = (
            id(&doc, "guild_id"),
            text(&doc, "channel_type").and_then(ChannelKind::parse),
            id(&doc, "channel_id"),
        );
        let imported = if let (Some(guild), Some(kind), Some(channel)) = parsed {
            if kind == ChannelKind::Notification {
                notification_guilds.push(guild);
            }
            sqlite
                .call(move |c| db::guild::set_channel(c, Id::new(guild), kind, Id::new(channel)))
                .await?;
            true
        } else {
            false
        };
        report.seen("channels", imported);
    }

    for doc in all(&mongo, "roles").await? {
        let parsed = (
            id(&doc, "guild_id"),
            text(&doc, "role_type").and_then(RoleKind::parse),
            id(&doc, "role_id"),
        );
        let imported = if let (Some(guild), Some(kind), Some(role)) = parsed {
            let binding = RoleBinding {
                id: Id::new(role),
                self_assignable: RoleKind::PICKABLE.contains(&kind)
                    && doc
                        .get_bool("self_assignable")
                        .unwrap_or(false),
            };
            sqlite
                .call(move |c| db::guild::set_role(c, Id::new(guild), kind, binding))
                .await?;
            true
        } else {
            false
        };
        report.seen("roles", imported);
    }

    for doc in all(&mongo, "guild_settings").await? {
        let imported = if let Some(guild) = id(&doc, "guild_id") {
            let enabled = doc
                .get_bool("scam_detect_enabled")
                .unwrap_or(false);
            sqlite
                .call(move |c| db::guild::set_scam_detect(c, Id::new(guild), enabled))
                .await?;
            true
        } else {
            false
        };
        report.seen("guild_settings", imported);
    }

    for doc in all(&mongo, "quarantines").await? {
        let released = doc
            .get_bool("released")
            .unwrap_or(false);
        let parsed = (
            id(&doc, "guild_id"),
            id(&doc, "user_id"),
            text(&doc, "token"),
        );
        let imported = match parsed {
            (Some(guild), Some(user), Some(token)) if !released => {
                let roles = doc
                    .get_array("roles")
                    .map(|roles| {
                        roles
                            .iter()
                            .filter_map(|r| match r {
                                Bson::Int64(v) => Id::new_checked(*v as u64),
                                Bson::Int32(v) => Id::new_checked(*v as u64),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                // Startup compares this with the join time: a later join re-applies, an earlier
                // one releases. Errs towards re-applying (0 always does).
                let created_at = doc
                    .get_object_id("_id")
                    .map_or(0, |id| id.timestamp().timestamp_millis() / 1000);
                let record = Record {
                    guild_id: Id::new(guild),
                    user_id: Id::new(user),
                    token: token.to_owned(),
                    roles,
                    state: State::Active,
                    reason: Reason::Spam,
                    evidence: "ย้ายข้อมูลจากระบบเดิม".to_owned(),
                    case_message: None,
                    created_at,
                    kicked: false,
                };
                sqlite
                    .call(move |c| db::quarantine::insert_new(c, &record))
                    .await?;
                true
            }
            _ => false,
        };
        report.seen("quarantines (active)", imported);
    }

    for doc in all(&mongo, "ai_prompts").await? {
        let parsed = (
            id(&doc, "user_id"),
            text(&doc, "prompt").map(str::to_owned),
        );
        let imported = if let (Some(user), Some(prompt)) = parsed {
            let now = db::now();
            sqlite
                .call(move |c| db::ai::set_prompt(c, Id::new(user), &prompt, now))
                .await?;
            true
        } else {
            false
        };
        report.seen("ai_prompts", imported);
    }

    for doc in all(&mongo, "messages").await? {
        let kind = match text(&doc, "message_type") {
            Some("status") => Some(BotMessageKind::Status),
            Some("role") => Some(BotMessageKind::RolePicker),
            _ => None,
        };
        let parsed = (
            id(&doc, "guild_id"),
            id(&doc, "channel_id"),
            id(&doc, "message_id"),
            kind,
        );
        let imported = if let (Some(guild), Some(channel), Some(message), Some(kind)) = parsed {
            let record = BotMessage {
                channel_id: Id::new(channel),
                message_id: Id::new(message),
                digest: None,
            };
            // With `--force`, keep the card the bot already manages.
            sqlite
                .call(move |c| {
                    if db::messages::get(c, Id::new(guild), kind)?.is_some() {
                        return Ok(false);
                    }
                    db::messages::set(c, Id::new(guild), kind, record)?;
                    Ok(true)
                })
                .await?
        } else {
            false
        };
        report.seen("bot messages", imported);
    }

    let now_ms = jiff::Timestamp::now().as_millisecond();
    let week = format!("w{}", cycles::week_index(now_ms));
    let mut kinds = vec!["helminth", "riven_sliver"];
    if cycles::steel_path(now_ms).umbra {
        kinds.push("umbra_forma");
    }
    for guild in notification_guilds {
        for kind in &kinds {
            let (kind, week) = (*kind, week.clone());
            sqlite
                .call(move |c| {
                    db::notify::mark_sent(c, Id::new(guild), kind, &week, None, db::now())
                })
                .await?;
            report.seen("notification markers seeded", true);
        }
    }

    println!("migrated into {}", target.display());
    for (name, (seen, imported)) in report.counts {
        println!("  {name:<28} {imported:>4} imported / {seen:>4} read");
    }
    Ok(())
}
