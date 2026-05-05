#![cfg(feature = "test-utils")]

use discord_bot::{
    platform::testkit::onboarding::InMemoryOnboardingPorts,
    slices::onboarding::{
        app::member_flow::plan_member_join,
        domain::{ChannelKind, JoinPlan, RoleKind},
    },
};

#[tokio::test]
async fn onboarding_slice_restores_quarantine_with_in_memory_ports() {
    let mut ports = InMemoryOnboardingPorts::default();
    ports
        .tokens
        .insert((1, 42), "token-1".into());
    ports
        .roles
        .insert((1, RoleKind::Quarantine), 7);
    ports
        .channels
        .insert((1, ChannelKind::Quarantine), 9);

    let plan = plan_member_join(&ports, 1, 42, false, false).await;

    assert_eq!(
        plan,
        JoinPlan::RestoreQuarantine { token: "token-1".into(), role_id: 7, channel_id: 9 }
    );
}

#[tokio::test]
async fn onboarding_slice_assigns_guest_when_no_quarantine_exists() {
    let mut ports = InMemoryOnboardingPorts::default();
    ports
        .roles
        .insert((1, RoleKind::Guest), 5);
    ports
        .channels
        .insert((1, ChannelKind::Introduction), 6);

    let plan = plan_member_join(&ports, 1, 24, false, false).await;

    assert_eq!(
        plan,
        JoinPlan::AssignGuest { role_id: 5, channel_id: 6 }
    );
}
