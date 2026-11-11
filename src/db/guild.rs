use std::collections::HashMap;
use std::sync::Arc;

use rusqlite::{Connection, params};
use twilight_model::id::{
    Id,
    marker::{ChannelMarker, GuildMarker, RoleMarker},
};

use super::{id_from_sql, id_to_sql};
use crate::guild::{ChannelKind, GuildConfig, RoleBinding, RoleKind};

pub fn load_all(
    conn: &mut Connection,
) -> rusqlite::Result<HashMap<Id<GuildMarker>, Arc<GuildConfig>>> {
    let mut map: HashMap<Id<GuildMarker>, GuildConfig> = HashMap::new();

    let mut stmt = conn.prepare("SELECT guild_id, kind, channel_id FROM guild_channel")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (guild, kind, channel) = row?;
        let (Some(guild), Some(kind), Some(channel)) = (
            Id::new_checked(id_from_sql(guild)),
            ChannelKind::parse(&kind),
            Id::new_checked(id_from_sql(channel)),
        ) else {
            tracing::warn!(
                guild,
                kind,
                channel,
                "skipping invalid channel row"
            );
            continue;
        };
        map.entry(guild)
            .or_default()
            .channels
            .insert(kind, channel);
    }

    let mut stmt =
        conn.prepare("SELECT guild_id, kind, role_id, self_assignable FROM guild_role")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, bool>(3)?,
        ))
    })?;
    for row in rows {
        let (guild, kind, role, self_assignable) = row?;
        let (Some(guild), Some(kind), Some(id)) = (
            Id::new_checked(id_from_sql(guild)),
            RoleKind::parse(&kind),
            Id::new_checked(id_from_sql(role)),
        ) else {
            tracing::warn!(guild, kind, role, "skipping invalid role row");
            continue;
        };
        map.entry(guild)
            .or_default()
            .roles
            .insert(kind, RoleBinding { id, self_assignable });
    }

    let mut stmt = conn.prepare("SELECT guild_id, scam_detect FROM guild_settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?))
    })?;
    for row in rows {
        let (guild, scam_detect) = row?;
        if let Some(guild) = Id::new_checked(id_from_sql(guild)) {
            map.entry(guild)
                .or_default()
                .scam_detect = scam_detect;
        }
    }

    Ok(map
        .into_iter()
        .map(|(guild, config)| (guild, Arc::new(config)))
        .collect())
}

pub fn set_channel(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: ChannelKind,
    channel: Id<ChannelMarker>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO guild_channel (guild_id, kind, channel_id) VALUES (?1, ?2, ?3)
         ON CONFLICT (guild_id, kind) DO UPDATE SET channel_id = excluded.channel_id",
        params![id_to_sql(guild.get()), kind.as_str(), id_to_sql(channel.get())],
    )?;
    Ok(())
}

pub fn clear_channel(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    channel: Id<ChannelMarker>,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM guild_channel WHERE guild_id = ?1 AND channel_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(channel.get())],
    )?;
    Ok(())
}

pub fn set_role(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: RoleKind,
    binding: RoleBinding,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO guild_role (guild_id, kind, role_id, self_assignable) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (guild_id, kind) DO UPDATE
         SET role_id = excluded.role_id, self_assignable = excluded.self_assignable",
        params![
            id_to_sql(guild.get()),
            kind.as_str(),
            id_to_sql(binding.id.get()),
            binding.self_assignable
        ],
    )?;
    Ok(())
}

pub fn clear_role(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    role: Id<RoleMarker>,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM guild_role WHERE guild_id = ?1 AND role_id = ?2",
        params![id_to_sql(guild.get()), id_to_sql(role.get())],
    )?;
    Ok(())
}

pub fn set_scam_detect(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    enabled: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO guild_settings (guild_id, scam_detect) VALUES (?1, ?2)
         ON CONFLICT (guild_id) DO UPDATE SET scam_detect = excluded.scam_detect",
        params![id_to_sql(guild.get()), enabled],
    )?;
    Ok(())
}

pub fn clear_channel_kind(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: ChannelKind,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM guild_channel WHERE guild_id = ?1 AND kind = ?2",
        params![id_to_sql(guild.get()), kind.as_str()],
    )?;
    Ok(())
}

pub fn clear_role_kind(
    conn: &mut Connection,
    guild: Id<GuildMarker>,
    kind: RoleKind,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM guild_role WHERE guild_id = ?1 AND kind = ?2",
        params![id_to_sql(guild.get()), kind.as_str()],
    )?;
    Ok(())
}
