use crate::slices::onboarding::{
    app::ports::OnboardingReadPorts,
    domain::{ChannelKind, JoinPlan, RoleKind},
};

pub async fn plan_member_join<P>(
    ports: &P,
    guild_id: u64,
    user_id: u64,
    is_bot: bool,
    is_system: bool,
) -> JoinPlan
where
    P: OnboardingReadPorts,
{
    if is_bot || is_system {
        return JoinPlan::Ignore;
    }

    if let (Some(token), Some(role_id), Some(channel_id)) = (
        ports
            .quarantine_token(guild_id, user_id)
            .await,
        ports
            .role_id(guild_id, RoleKind::Quarantine)
            .await,
        ports
            .channel_id(guild_id, ChannelKind::Quarantine)
            .await,
    ) {
        return JoinPlan::RestoreQuarantine { token, role_id, channel_id };
    }

    if let (Some(role_id), Some(channel_id)) = (
        ports
            .role_id(guild_id, RoleKind::Guest)
            .await,
        ports
            .channel_id(guild_id, ChannelKind::Introduction)
            .await,
    ) {
        return JoinPlan::AssignGuest { role_id, channel_id };
    }

    JoinPlan::Noop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ignores_bot_users() {
        let ports = crate::slices::onboarding::app::ports::MockOnboardingReadPorts::new();
        let plan = plan_member_join(&ports, 1, 2, true, false).await;
        assert_eq!(plan, JoinPlan::Ignore);
    }

    #[tokio::test]
    async fn prefers_restoring_quarantine() {
        let mut ports = crate::slices::onboarding::app::ports::MockOnboardingReadPorts::new();
        ports
            .expect_quarantine_token()
            .returning(|_, _| Some("token".to_string()));
        ports
            .expect_role_id()
            .returning(|_, role| match role {
                RoleKind::Quarantine => Some(10),
                RoleKind::Guest => Some(20),
            });
        ports
            .expect_channel_id()
            .returning(|_, channel| match channel {
                ChannelKind::Quarantine => Some(30),
                ChannelKind::Introduction => Some(40),
            });

        let plan = plan_member_join(&ports, 1, 2, false, false).await;
        assert_eq!(
            plan,
            JoinPlan::RestoreQuarantine { token: "token".into(), role_id: 10, channel_id: 30 }
        );
    }

    #[tokio::test]
    async fn falls_back_to_guest_assignment() {
        let mut ports = crate::slices::onboarding::app::ports::MockOnboardingReadPorts::new();
        ports
            .expect_quarantine_token()
            .returning(|_, _| None);
        ports
            .expect_role_id()
            .returning(|_, role| match role {
                RoleKind::Quarantine => None,
                RoleKind::Guest => Some(20),
            });
        ports
            .expect_channel_id()
            .returning(|_, channel| match channel {
                ChannelKind::Quarantine => None,
                ChannelKind::Introduction => Some(40),
            });

        let plan = plan_member_join(&ports, 1, 2, false, false).await;
        assert_eq!(
            plan,
            JoinPlan::AssignGuest { role_id: 20, channel_id: 40 }
        );
    }
}
