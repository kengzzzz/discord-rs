use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, RwLock},
};

use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker},
};

use crate::db::{self, Db};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    Notification,
    Status,
    Introduction,
    RolePicker,
    Relay,
    RelayB1,
    RelayB2,
    Quarantine,
    VoiceLog,
    ModLog,
}

impl ChannelKind {
    pub const ALL: [Self; 10] = [
        Self::Notification,
        Self::Status,
        Self::Introduction,
        Self::RolePicker,
        Self::Relay,
        Self::RelayB1,
        Self::RelayB2,
        Self::Quarantine,
        Self::VoiceLog,
        Self::ModLog,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Notification => "notification",
            Self::Status => "status",
            Self::Introduction => "introduction",
            Self::RolePicker => "update_role",
            Self::Relay => "broadcast",
            Self::RelayB1 => "broadcast_b1",
            Self::RelayB2 => "broadcast_b2",
            Self::Quarantine => "quarantine",
            Self::VoiceLog => "log",
            Self::ModLog => "mod_log",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Notification => "แจ้งเตือน",
            Self::Status => "สถานะ Warframe",
            Self::Introduction => "แนะนำตัว",
            Self::RolePicker => "เลือก Role",
            Self::Relay => "Relay (กลุ่มหลัก)",
            Self::RelayB1 => "Relay (กลุ่ม 1)",
            Self::RelayB2 => "Relay (กลุ่ม 2)",
            Self::Quarantine => "กักกัน",
            Self::VoiceLog => "Voice log",
            Self::ModLog => "Moderation log",
        }
    }

    pub const fn is_relay(self) -> bool {
        matches!(self, Self::Relay | Self::RelayB1 | Self::RelayB2)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RoleKind {
    Guest,
    Member,
    Quarantine,
    Helminth,
    RivenSliver,
    UmbraForma,
    Baro,
    CetusNight,
    Eidolon,
    Live,
    Warframe,
    Soulframe,
}

impl RoleKind {
    pub const ALL: [Self; 12] = [
        Self::Guest,
        Self::Member,
        Self::Quarantine,
        Self::Helminth,
        Self::RivenSliver,
        Self::UmbraForma,
        Self::Baro,
        Self::CetusNight,
        Self::Eidolon,
        Self::Live,
        Self::Warframe,
        Self::Soulframe,
    ];

    pub const PICKABLE: [Self; 9] = [
        Self::Helminth,
        Self::RivenSliver,
        Self::UmbraForma,
        Self::Baro,
        Self::CetusNight,
        Self::Eidolon,
        Self::Live,
        Self::Warframe,
        Self::Soulframe,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Guest => "guest",
            Self::Member => "member",
            Self::Quarantine => "quarantine",
            Self::Helminth => "helminth",
            Self::RivenSliver => "riven_silver",
            Self::UmbraForma => "umbral_forma",
            Self::Baro => "baro",
            Self::CetusNight => "cetus_night",
            Self::Eidolon => "eidolon",
            Self::Live => "live",
            Self::Warframe => "warframe",
            Self::Soulframe => "soulframe",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Guest => "Guest",
            Self::Member => "Member",
            Self::Quarantine => "กักกัน",
            Self::Helminth => "Helminth",
            Self::RivenSliver => "Riven Sliver",
            Self::UmbraForma => "Umbra Forma",
            Self::Baro => "Baro Ki'Teer",
            Self::CetusNight => "Cetus กลางคืน",
            Self::Eidolon => "Eidolon",
            Self::Live => "Live",
            Self::Warframe => "Warframe",
            Self::Soulframe => "Soulframe",
        }
    }

    pub const fn hint(self) -> &'static str {
        match self {
            Self::Helminth => "แจ้งเตือนบัพ Helminth ทุกวันจันทร์",
            Self::RivenSliver => "แจ้งเตือนแลก Riven Sliver ทุกวันจันทร์",
            Self::UmbraForma => "แจ้งเตือนเมื่อ Steel Path มี Umbra Forma",
            Self::Baro => "แจ้งเตือนเมื่อ Baro Ki'Teer มาถึง",
            Self::CetusNight => "แจ้งเตือน Cetus กลางคืน (ทุก 2.5 ชม.)",
            Self::Eidolon => "หาทีมล่า Eidolon",
            Self::Live => "แจ้งเตือนไลฟ์",
            Self::Warframe => "ข่าวสาร Warframe",
            Self::Soulframe => "ข่าวสาร Soulframe",
            Self::Guest | Self::Member | Self::Quarantine => "",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleBinding {
    pub id: Id<RoleMarker>,
    pub self_assignable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuildConfig {
    pub channels: HashMap<ChannelKind, Id<ChannelMarker>>,
    pub roles: HashMap<RoleKind, RoleBinding>,
    pub scam_detect: bool,
}

impl GuildConfig {
    pub fn channel(&self, kind: ChannelKind) -> Option<Id<ChannelMarker>> {
        self.channels.get(&kind).copied()
    }

    pub fn role(&self, kind: RoleKind) -> Option<RoleBinding> {
        self.roles.get(&kind).copied()
    }

    pub fn role_id(&self, kind: RoleKind) -> Option<Id<RoleMarker>> {
        self.roles
            .get(&kind)
            .map(|binding| binding.id)
    }

    pub fn kinds_of(&self, channel: Id<ChannelMarker>) -> impl Iterator<Item = ChannelKind> + '_ {
        self.channels
            .iter()
            .filter(move |(_, id)| **id == channel)
            .map(|(kind, _)| *kind)
    }

    pub fn is(&self, channel: Id<ChannelMarker>, kind: ChannelKind) -> bool {
        self.channel(kind) == Some(channel)
    }

    pub fn pickable_role(&self, role: Id<RoleMarker>) -> Option<RoleKind> {
        RoleKind::PICKABLE
            .into_iter()
            .find(|kind| {
                self.roles
                    .get(kind)
                    .is_some_and(|b| b.id == role && b.self_assignable)
            })
    }
}

static EMPTY: LazyLock<Arc<GuildConfig>> = LazyLock::new(Arc::default);

pub struct Guilds {
    db: Db,
    map: RwLock<HashMap<Id<GuildMarker>, Arc<GuildConfig>>>,
    /// Keeps database and memory updates in the same order.
    writes: tokio::sync::Mutex<()>,
}

impl Guilds {
    pub async fn load(db: Db) -> anyhow::Result<Self> {
        let map = db.call(db::guild::load_all).await?;
        Ok(Self { db, map: RwLock::new(map), writes: tokio::sync::Mutex::new(()) })
    }

    pub fn get(&self, guild: Id<GuildMarker>) -> Arc<GuildConfig> {
        self.read()
            .get(&guild)
            .cloned()
            .unwrap_or_else(|| EMPTY.clone())
    }

    pub fn with_channel(&self, kind: ChannelKind) -> Vec<(Id<GuildMarker>, Id<ChannelMarker>)> {
        self.read()
            .iter()
            .filter_map(|(guild, config)| {
                config
                    .channel(kind)
                    .map(|channel| (*guild, channel))
            })
            .collect()
    }

    pub fn guild_ids(&self) -> Vec<Id<GuildMarker>> {
        self.read().keys().copied().collect()
    }

    pub async fn set_channel(
        &self,
        guild: Id<GuildMarker>,
        kind: ChannelKind,
        channel: Id<ChannelMarker>,
    ) -> anyhow::Result<()> {
        let _write = self.writes.lock().await;
        self.db
            .call(move |c| db::guild::set_channel(c, guild, kind, channel))
            .await?;
        self.update(guild, |config| {
            config.channels.insert(kind, channel);
        });
        Ok(())
    }

    pub async fn clear_channel(
        &self,
        guild: Id<GuildMarker>,
        channel: Id<ChannelMarker>,
    ) -> anyhow::Result<Vec<ChannelKind>> {
        let _write = self.writes.lock().await;
        let cleared: Vec<ChannelKind> = self
            .get(guild)
            .kinds_of(channel)
            .collect();
        self.db
            .call(move |c| db::guild::clear_channel(c, guild, channel))
            .await?;
        self.update(guild, |config| {
            config
                .channels
                .retain(|_, id| *id != channel)
        });
        Ok(cleared)
    }

    pub async fn clear_kind(
        &self,
        guild: Id<GuildMarker>,
        kind: ChannelKind,
    ) -> anyhow::Result<()> {
        let _write = self.writes.lock().await;
        self.db
            .call(move |c| db::guild::clear_channel_kind(c, guild, kind))
            .await?;
        self.update(guild, |config| {
            config.channels.remove(&kind);
        });
        Ok(())
    }

    pub async fn set_role(
        &self,
        guild: Id<GuildMarker>,
        kind: RoleKind,
        binding: RoleBinding,
    ) -> anyhow::Result<()> {
        let _write = self.writes.lock().await;
        self.db
            .call(move |c| db::guild::set_role(c, guild, kind, binding))
            .await?;
        self.update(guild, |config| {
            config.roles.insert(kind, binding);
        });
        Ok(())
    }

    pub async fn clear_role(
        &self,
        guild: Id<GuildMarker>,
        role: Id<RoleMarker>,
    ) -> anyhow::Result<Vec<RoleKind>> {
        let _write = self.writes.lock().await;
        let cleared: Vec<RoleKind> = self
            .get(guild)
            .roles
            .iter()
            .filter(|(_, binding)| binding.id == role)
            .map(|(kind, _)| *kind)
            .collect();
        self.db
            .call(move |c| db::guild::clear_role(c, guild, role))
            .await?;
        self.update(guild, |config| {
            config
                .roles
                .retain(|_, binding| binding.id != role)
        });
        Ok(cleared)
    }

    pub async fn clear_role_kind(
        &self,
        guild: Id<GuildMarker>,
        kind: RoleKind,
    ) -> anyhow::Result<()> {
        let _write = self.writes.lock().await;
        self.db
            .call(move |c| db::guild::clear_role_kind(c, guild, kind))
            .await?;
        self.update(guild, |config| {
            config.roles.remove(&kind);
        });
        Ok(())
    }

    pub async fn set_scam_detect(
        &self,
        guild: Id<GuildMarker>,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let _write = self.writes.lock().await;
        self.db
            .call(move |c| db::guild::set_scam_detect(c, guild, enabled))
            .await?;
        self.update(guild, |config| config.scam_detect = enabled);
        Ok(())
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<Id<GuildMarker>, Arc<GuildConfig>>> {
        self.map
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn update(&self, guild: Id<GuildMarker>, f: impl FnOnce(&mut GuildConfig)) {
        let mut map = self
            .map
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = map.entry(guild).or_default();
        f(Arc::make_mut(entry));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_through_storage_names() {
        for kind in ChannelKind::ALL {
            assert_eq!(ChannelKind::parse(kind.as_str()), Some(kind));
        }
        for kind in RoleKind::ALL {
            assert_eq!(RoleKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_changes_leave_memory_matching_storage() {
        let db = Db::open_in_memory().unwrap();
        let guilds = Arc::new(Guilds::load(db.clone()).await.unwrap());
        let guild = Id::new(1);
        for round in 0..2000u64 {
            let tasks: Vec<_> = (0..4u64)
                .map(|i| {
                    let guilds = Arc::clone(&guilds);
                    tokio::spawn(async move {
                        guilds
                            .set_channel(
                                guild,
                                ChannelKind::Status,
                                Id::new(round * 10 + i + 1),
                            )
                            .await
                    })
                })
                .collect();
            for task in tasks {
                task.await.unwrap().unwrap();
            }
            let stored = Guilds::load(db.clone())
                .await
                .unwrap()
                .get(guild)
                .channel(ChannelKind::Status);
            assert_eq!(
                guilds
                    .get(guild)
                    .channel(ChannelKind::Status),
                stored,
                "round {round}"
            );
        }
    }

    #[tokio::test]
    async fn config_writes_persist_and_reload() {
        let db = Db::open_in_memory().unwrap();
        let guilds = Guilds::load(db.clone()).await.unwrap();
        let guild = Id::new(1);
        let channel = Id::new(10);
        guilds
            .set_channel(guild, ChannelKind::Status, channel)
            .await
            .unwrap();
        guilds
            .set_channel(guild, ChannelKind::Relay, channel)
            .await
            .unwrap();
        guilds
            .set_role(
                guild,
                RoleKind::Helminth,
                RoleBinding { id: Id::new(5), self_assignable: true },
            )
            .await
            .unwrap();
        guilds
            .set_scam_detect(guild, true)
            .await
            .unwrap();

        let reloaded = Guilds::load(db)
            .await
            .unwrap()
            .get(guild);
        assert_eq!(
            reloaded.channel(ChannelKind::Status),
            Some(channel)
        );
        assert_eq!(
            reloaded.pickable_role(Id::new(5)),
            Some(RoleKind::Helminth)
        );
        assert!(reloaded.scam_detect);

        let mut cleared = guilds
            .clear_channel(guild, channel)
            .await
            .unwrap();
        cleared.sort_by_key(|k| k.as_str());
        assert_eq!(
            cleared,
            vec![ChannelKind::Relay, ChannelKind::Status]
        );
        assert!(guilds.get(guild).channels.is_empty());
    }
}
