#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RoleKind {
    Guest,
    Quarantine,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChannelKind {
    Introduction,
    Quarantine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JoinPlan {
    Ignore,
    RestoreQuarantine { token: String, role_id: u64, channel_id: u64 },
    QuarantineConfigIncomplete { token: String, missing_role: bool, missing_channel: bool },
    AssignGuest { role_id: u64, channel_id: u64 },
    Noop,
}
