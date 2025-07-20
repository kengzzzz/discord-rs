use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::{
    Interaction, InteractionData, InteractionType,
    application_command::{CommandData, CommandDataOption},
};
use twilight_model::guild::{MemberFlags, PartialMember};
use twilight_model::id::{
    Id,
    marker::{ApplicationMarker, CommandMarker, GuildMarker, InteractionMarker, UserMarker},
};
use twilight_model::oauth::ApplicationIntegrationMap;
use twilight_model::user::User;

pub fn command_interaction(
    command_name: &str,
    guild_id: Option<u64>,
) -> (Interaction, CommandData) {
    let application_id = Id::<ApplicationMarker>::new(1);
    let interaction_id = Id::<InteractionMarker>::new(100);
    let command_id = Id::<CommandMarker>::new(1);
    let user_id = Id::<UserMarker>::new(200);

    let user = User {
        accent_color: None,
        avatar: None,
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator: 0,
        email: None,
        flags: None,
        global_name: None,
        id: user_id,
        locale: None,
        mfa_enabled: None,
        name: "tester".into(),
        premium_type: None,
        public_flags: None,
        system: None,
        verified: None,
    };

    let member = PartialMember {
        avatar: None,
        communication_disabled_until: None,
        deaf: false,
        flags: MemberFlags::empty(),
        joined_at: None,
        mute: false,
        nick: None,
        permissions: None,
        premium_since: None,
        roles: Vec::new(),
        user: Some(user.clone()),
    };

    let command = CommandData {
        guild_id: guild_id.map(Id::new),
        id: command_id,
        name: command_name.to_owned(),
        kind: CommandType::ChatInput,
        options: Vec::new(),
        resolved: None,
        target_id: None,
    };

    let interaction = Interaction {
        app_permissions: None,
        application_id,
        authorizing_integration_owners: ApplicationIntegrationMap {
            guild: None,
            user: None,
        },
        channel: None,
        #[allow(deprecated)]
        channel_id: None,
        context: None,
        data: Some(InteractionData::ApplicationCommand(Box::new(
            command.clone(),
        ))),
        entitlements: Vec::new(),
        guild: None,
        guild_id: guild_id.map(Id::new),
        guild_locale: None,
        id: interaction_id,
        kind: InteractionType::ApplicationCommand,
        locale: None,
        member: guild_id.map(|_| member),
        message: None,
        token: "token".into(),
        user: guild_id.map(|_| user.clone()).is_none().then_some(user),
    };

    (interaction, command)
}

pub fn command_interaction_with_options(
    command_name: &str,
    guild_id: Option<u64>,
    options: Vec<CommandDataOption>,
) -> (Interaction, CommandData) {
    let (mut interaction, mut data) = command_interaction(command_name, guild_id);
    data.options = options;
    interaction.data = Some(InteractionData::ApplicationCommand(Box::new(data.clone())));
    (interaction, data)
}
