use async_trait::async_trait;

use crate::slices::onboarding::domain::{ChannelKind, RoleKind};

#[cfg_attr(any(test, feature = "test-utils"), mockall::automock)]
#[async_trait]
pub trait OnboardingReadPorts: Send + Sync {
    async fn quarantine_token(&self, guild_id: u64, user_id: u64) -> Option<String>;
    async fn role_id(&self, guild_id: u64, role: RoleKind) -> Option<u64>;
    async fn channel_id(&self, guild_id: u64, channel: ChannelKind) -> Option<u64>;
}
