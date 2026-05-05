use std::collections::HashMap;

use async_trait::async_trait;

use crate::slices::onboarding::{
    app::ports::OnboardingReadPorts,
    domain::{ChannelKind, RoleKind},
};

#[derive(Default)]
pub struct InMemoryOnboardingPorts {
    pub tokens: HashMap<(u64, u64), String>,
    pub roles: HashMap<(u64, RoleKind), u64>,
    pub channels: HashMap<(u64, ChannelKind), u64>,
}

#[async_trait]
impl OnboardingReadPorts for InMemoryOnboardingPorts {
    async fn quarantine_token(&self, guild_id: u64, user_id: u64) -> Option<String> {
        self.tokens
            .get(&(guild_id, user_id))
            .cloned()
    }

    async fn role_id(&self, guild_id: u64, role: RoleKind) -> Option<u64> {
        self.roles
            .get(&(guild_id, role))
            .copied()
    }

    async fn channel_id(&self, guild_id: u64, channel: ChannelKind) -> Option<u64> {
        self.channels
            .get(&(guild_id, channel))
            .copied()
    }
}
