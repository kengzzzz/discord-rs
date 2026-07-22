use mongodb::{
    bson::{Bson, doc, from_document, oid::ObjectId, to_bson},
    change_stream::event::ChangeStreamEvent,
};
use serde::{Serialize, de::DeserializeOwned};

use super::*;
use crate::{
    dbs::mongo::models::{channel::ChannelEnum, role::RoleEnum},
    dbs::redis::{new_pool, redis_delete, redis_exists, redis_set_ex},
};

fn event<T>(operation: &str, current: Option<T>, previous: Option<T>) -> ChangeStreamEvent<T>
where
    T: Serialize + DeserializeOwned,
{
    let mut event = doc! {
        "_id": { "_data": format!("test-{operation}") },
        "operationType": operation,
        "documentKey": { "_id": ObjectId::new() },
    };
    if let Some(current) = current {
        event.insert(
            "fullDocument",
            to_bson(&current).expect("serialize current document"),
        );
    }
    if let Some(previous) = previous {
        event.insert(
            "fullDocumentBeforeChange",
            to_bson(&previous).expect("serialize previous document"),
        );
    }
    from_document(event).expect("deserialize change event")
}

async fn seed(pool: &Pool, keys: &[&str]) {
    for key in keys {
        redis_set_ex(pool, key, &"cached", 60).await;
    }
}

async fn assert_missing(pool: &Pool, keys: &[&str]) {
    for key in keys {
        assert!(
            !redis_exists(pool, key).await,
            "{key} should be deleted"
        );
    }
}

#[test]
fn invalidation_requires_the_images_needed_for_targeted_eviction() {
    assert!(matches!(
        invalidation_for(event::<Bson>("delete", None, None)),
        Invalidation::Sweep(OperationType::Delete)
    ));
    assert!(matches!(
        invalidation_for(event("update", Some(Bson::Int32(1)), None)),
        Invalidation::Sweep(OperationType::Update)
    ));
    assert!(matches!(
        invalidation_for(event("replace", None, Some(Bson::Int32(1)))),
        Invalidation::Sweep(OperationType::Replace)
    ));
    assert!(matches!(
        invalidation_for(event("insert", Some(Bson::Int32(1)), None)),
        Invalidation::Documents(documents) if documents == [Bson::Int32(1)]
    ));
    assert!(matches!(
        invalidation_for(event::<Bson>("drop", None, None)),
        Invalidation::Ignore
    ));
}

#[tokio::test]
async fn delete_without_preimage_sweeps_each_collections_cache_families() {
    let pool = new_pool();
    let channel_keys = [
        "discord-bot:channel:91001",
        "discord-bot:channel-type:91:status",
        "discord-bot:channels-by-type:status",
    ];
    let role_keys = ["discord-bot:role:92001", "discord-bot:role-type:92:member"];
    let quarantine_keys =
        ["spam:quarantine:93:93001", "spam:log:93:93001", "spam:campaign:93:93001:hash"];
    let message_keys = ["discord-bot:role-message:94", "discord-bot:status-message:94"];
    let ai_keys = ["discord-bot:ai:prompt:95001", "discord-bot:ai:history:95001"];
    let guild_settings_keys = ["discord-bot:guild-settings:96"];
    let preserved_keys =
        ["discord-bot:wf:news", "discord-bot:ai:rate:95001", "changestream:resume:test-fallback"];

    for keys in [
        channel_keys.as_slice(),
        role_keys.as_slice(),
        quarantine_keys.as_slice(),
        message_keys.as_slice(),
        ai_keys.as_slice(),
        guild_settings_keys.as_slice(),
        preserved_keys.as_slice(),
    ] {
        seed(&pool, keys).await;
    }

    handle_channel_event(&pool, event("delete", None, None)).await;
    handle_role_event(&pool, event("delete", None, None)).await;
    handle_quarantine_event(&pool, event("delete", None, None)).await;
    handle_message_event(&pool, event("delete", None, None)).await;
    handle_ai_prompt_event(&pool, event("delete", None, None)).await;
    handle_guild_settings_event(&pool, event("delete", None, None)).await;

    for keys in [
        channel_keys.as_slice(),
        role_keys.as_slice(),
        quarantine_keys.as_slice(),
        message_keys.as_slice(),
        ai_keys.as_slice(),
        guild_settings_keys.as_slice(),
    ] {
        assert_missing(&pool, keys).await;
    }
    for key in preserved_keys {
        assert!(
            redis_exists(&pool, key).await,
            "{key} should be preserved"
        );
        redis_delete(&pool, key).await;
    }
}

#[tokio::test]
async fn continuity_recovery_sweeps_each_collections_cache_families() {
    let pool = new_pool();
    let watched_keys = [
        "discord-bot:channel:91101",
        "discord-bot:channel-type:911:status",
        "discord-bot:channels-by-type:status",
        "discord-bot:role:91201",
        "discord-bot:role-type:912:member",
        "spam:quarantine:913:91301",
        "spam:log:913:91301",
        "spam:campaign:913:91301:hash",
        "discord-bot:role-message:914",
        "discord-bot:status-message:914",
        "discord-bot:ai:prompt:91501",
        "discord-bot:ai:history:91501",
        "discord-bot:guild-settings:916",
    ];
    let preserved_keys = [
        "discord-bot:wf:news",
        "discord-bot:ai:rate:91501",
        "changestream:resume:test-continuity-recovery",
    ];
    seed(&pool, &watched_keys).await;
    seed(&pool, &preserved_keys).await;

    let mut deleted = 0;
    for prefixes in [
        channel_cache_prefixes(),
        role_cache_prefixes(),
        quarantine_cache_prefixes(),
        message_cache_prefixes(),
        ai_prompt_cache_prefixes(),
        guild_settings_cache_prefixes(),
    ] {
        deleted += redis_delete_prefixes_checked(&pool, &prefixes)
            .await
            .expect("purge cache family");
    }

    assert_eq!(deleted, watched_keys.len());
    assert_missing(&pool, &watched_keys).await;
    for key in preserved_keys {
        assert!(
            redis_exists(&pool, key).await,
            "{key} should be preserved"
        );
        redis_delete(&pool, key).await;
    }
}

#[tokio::test]
async fn delete_with_preimage_keeps_channel_invalidation_targeted() {
    let pool = new_pool();
    let target_keys = [
        "discord-bot:channel:97001",
        "discord-bot:channel-type:97:status",
        "discord-bot:channels-by-type:status",
    ];
    let sibling_keys = [
        "discord-bot:channel:97002",
        "discord-bot:channel-type:98:notification",
        "discord-bot:channels-by-type:notification",
    ];
    seed(&pool, &target_keys).await;
    seed(&pool, &sibling_keys).await;

    let previous =
        Channel { id: None, channel_type: ChannelEnum::Status, channel_id: 97001, guild_id: 97 };
    handle_channel_event(&pool, event("delete", None, Some(previous))).await;

    assert_missing(&pool, &target_keys).await;
    for key in sibling_keys {
        assert!(
            redis_exists(&pool, key).await,
            "{key} should be preserved"
        );
        redis_delete(&pool, key).await;
    }
}

#[tokio::test]
async fn complete_update_targets_old_and_new_role_keys() {
    let pool = new_pool();
    let target_keys = [
        "discord-bot:role:98001",
        "discord-bot:role-type:98:member",
        "discord-bot:role:98002",
        "discord-bot:role-type:98:guest",
    ];
    let sibling_key = "discord-bot:role:98003";
    seed(&pool, &target_keys).await;
    seed(&pool, &[sibling_key]).await;

    let previous = Role {
        id: None,
        role_type: RoleEnum::Member,
        role_id: 98001,
        guild_id: 98,
        self_assignable: false,
    };
    let current = Role {
        id: None,
        role_type: RoleEnum::Guest,
        role_id: 98002,
        guild_id: 98,
        self_assignable: true,
    };
    handle_role_event(
        &pool,
        event("update", Some(current), Some(previous)),
    )
    .await;

    assert_missing(&pool, &target_keys).await;
    assert!(redis_exists(&pool, sibling_key).await);
    redis_delete(&pool, sibling_key).await;
}

#[tokio::test]
async fn update_without_preimage_sweeps_old_role_keys() {
    let pool = new_pool();
    let old_key = "discord-bot:role:99001";
    let unrelated_key = "discord-bot:status-message:99";
    seed(&pool, &[old_key, unrelated_key]).await;

    let current = Role {
        id: None,
        role_type: RoleEnum::Member,
        role_id: 99002,
        guild_id: 99,
        self_assignable: false,
    };
    handle_role_event(&pool, event("update", Some(current), None)).await;

    assert!(!redis_exists(&pool, old_key).await);
    assert!(redis_exists(&pool, unrelated_key).await);
    redis_delete(&pool, unrelated_key).await;
}
