use std::{mem, sync::Arc};

use async_trait::async_trait;
use once_cell::sync::Lazy;
use twilight_gateway::Event;
use twilight_model::{
    application::{
        command::Command,
        interaction::{
            Interaction, InteractionData, InteractionType, application_command::CommandData,
            message_component::MessageComponentInteractionData, modal::ModalInteractionData,
        },
    },
    channel::Message,
    gateway::payload::incoming::{
        GuildCreate, MemberAdd, MemberRemove, MessageDelete, MessageDeleteBulk, ReactionAdd,
        ReactionRemove, Ready, VoiceStateUpdate,
    },
};

use crate::{
    context::Context,
    features::{
        admin::AdminFeature, ai::AiFeature, help::HelpFeature, intro::IntroFeature,
        member_onboarding::MemberOnboardingFeature, message_pipeline::MessagePipelineFeature,
        ping::PingFeature, reaction_roles::ReactionRolesFeature,
        role_messages::RoleMessagesFeature, runtime::RuntimeFeature,
        verification::VerificationFeature, voice_logs::VoiceLogsFeature, warframe::WarframeFeature,
    },
};

#[async_trait]
pub trait FeatureSlice: Send + Sync {
    fn register_commands(&self, _commands: &mut Vec<Command>) {}

    fn command_names(&self) -> &'static [&'static str] {
        &[]
    }

    fn autocomplete_names(&self) -> &'static [&'static str] {
        &[]
    }

    fn modal_ids(&self) -> &'static [&'static str] {
        &[]
    }

    fn component_prefixes(&self) -> &'static [&'static str] {
        &[]
    }

    async fn handle_command(
        &self,
        _ctx: Arc<Context>,
        _interaction: Interaction,
        _data: CommandData,
    ) {
    }

    async fn handle_autocomplete(
        &self,
        _ctx: Arc<Context>,
        _interaction: Interaction,
        _data: CommandData,
    ) {
    }

    async fn handle_modal(
        &self,
        _ctx: Arc<Context>,
        _interaction: Interaction,
        _data: ModalInteractionData,
    ) {
    }

    async fn handle_component(
        &self,
        _ctx: Arc<Context>,
        _interaction: Interaction,
        _data: MessageComponentInteractionData,
    ) {
    }

    async fn handle_message_create(&self, _ctx: Arc<Context>, _message: Message) -> bool {
        false
    }

    async fn handle_reaction_add(&self, _ctx: Arc<Context>, _event: ReactionAdd) -> bool {
        false
    }

    async fn handle_reaction_remove(&self, _ctx: Arc<Context>, _event: ReactionRemove) -> bool {
        false
    }

    async fn handle_member_add(&self, _ctx: Arc<Context>, _event: MemberAdd) -> bool {
        false
    }

    async fn handle_member_remove(&self, _ctx: Arc<Context>, _event: MemberRemove) -> bool {
        false
    }

    async fn handle_message_delete(&self, _ctx: Arc<Context>, _event: MessageDelete) -> bool {
        false
    }

    async fn handle_message_delete_bulk(
        &self,
        _ctx: Arc<Context>,
        _event: MessageDeleteBulk,
    ) -> bool {
        false
    }

    async fn handle_ready(&self, _ctx: Arc<Context>, _event: Ready) -> bool {
        false
    }

    async fn handle_guild_create(&self, _ctx: Arc<Context>, _event: GuildCreate) -> bool {
        false
    }

    async fn handle_voice_state_update(
        &self,
        _ctx: Arc<Context>,
        _event: VoiceStateUpdate,
    ) -> bool {
        false
    }
}

pub struct FeatureRegistry {
    slices: Vec<Box<dyn FeatureSlice>>,
}

impl FeatureRegistry {
    fn new() -> Self {
        Self {
            slices: vec![
                Box::new(AdminFeature),
                Box::new(AiFeature),
                Box::new(HelpFeature),
                Box::new(IntroFeature),
                Box::new(MemberOnboardingFeature),
                Box::new(MessagePipelineFeature),
                Box::new(PingFeature),
                Box::new(ReactionRolesFeature),
                Box::new(RoleMessagesFeature),
                Box::new(RuntimeFeature),
                Box::new(VerificationFeature),
                Box::new(VoiceLogsFeature),
                Box::new(WarframeFeature),
            ],
        }
    }

    pub fn collect_commands(&self) -> Vec<Command> {
        let mut commands = Vec::new();
        for slice in &self.slices {
            slice.register_commands(&mut commands);
        }
        commands
    }

    fn slice_for_command(&self, name: &str) -> Option<&dyn FeatureSlice> {
        self.slices
            .iter()
            .map(Box::as_ref)
            .find(|slice| slice.command_names().contains(&name))
    }

    fn slice_for_autocomplete(&self, name: &str) -> Option<&dyn FeatureSlice> {
        self.slices
            .iter()
            .map(Box::as_ref)
            .find(|slice| {
                slice
                    .autocomplete_names()
                    .contains(&name)
            })
    }

    fn slice_for_modal(&self, modal_id: &str) -> Option<&dyn FeatureSlice> {
        self.slices
            .iter()
            .map(Box::as_ref)
            .find(|slice| slice.modal_ids().contains(&modal_id))
    }

    fn slice_for_component(&self, custom_id: &str) -> Option<&dyn FeatureSlice> {
        self.slices
            .iter()
            .map(Box::as_ref)
            .find(|slice| {
                slice
                    .component_prefixes()
                    .iter()
                    .any(|prefix| custom_id.starts_with(prefix))
            })
    }

    pub async fn handle_interaction(&self, ctx: Arc<Context>, interaction: Interaction) {
        let Some(user) = &interaction.author() else {
            return;
        };
        if user.bot || user.system.unwrap_or_default() {
            return;
        }

        let mut interaction = interaction;

        match mem::take(&mut interaction.data) {
            Some(InteractionData::ApplicationCommand(data)) => {
                if interaction.kind == InteractionType::ApplicationCommandAutocomplete {
                    if let Some(slice) = self.slice_for_autocomplete(&data.name) {
                        slice
                            .handle_autocomplete(ctx, interaction, *data)
                            .await;
                    }
                    return;
                }

                if let Some(slice) = self.slice_for_command(&data.name) {
                    slice
                        .handle_command(ctx, interaction, *data)
                        .await;
                }
            }
            Some(InteractionData::MessageComponent(data)) => {
                if let Some(slice) = self.slice_for_component(&data.custom_id) {
                    slice
                        .handle_component(ctx, interaction, *data)
                        .await;
                }
            }
            Some(InteractionData::ModalSubmit(data)) => {
                if let Some(slice) = self.slice_for_modal(&data.custom_id) {
                    slice
                        .handle_modal(ctx, interaction, *data)
                        .await;
                }
            }
            _ => {
                tracing::warn!("ignoring non-command interaction");
            }
        }
    }

    pub async fn dispatch_event(&self, ctx: Arc<Context>, event: Event) {
        match event {
            Event::MessageCreate(boxed) => {
                let message = (*boxed).0;
                for slice in &self.slices {
                    if slice
                        .handle_message_create(ctx.clone(), message.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::ReactionAdd(boxed) => {
                let event = *boxed;
                for slice in &self.slices {
                    if slice
                        .handle_reaction_add(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::ReactionRemove(boxed) => {
                let event = *boxed;
                for slice in &self.slices {
                    if slice
                        .handle_reaction_remove(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::MemberAdd(boxed) => {
                let event = *boxed;
                for slice in &self.slices {
                    if slice
                        .handle_member_add(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::MemberRemove(event) => {
                for slice in &self.slices {
                    if slice
                        .handle_member_remove(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::MessageDelete(event) => {
                for slice in &self.slices {
                    if slice
                        .handle_message_delete(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::MessageDeleteBulk(event) => {
                for slice in &self.slices {
                    if slice
                        .handle_message_delete_bulk(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::Ready(event) => {
                for slice in &self.slices {
                    if slice
                        .handle_ready(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::GuildCreate(boxed) => {
                let event = *boxed;
                for slice in &self.slices {
                    if slice
                        .handle_guild_create(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            Event::VoiceStateUpdate(boxed) => {
                let event = *boxed;
                for slice in &self.slices {
                    if slice
                        .handle_voice_state_update(ctx.clone(), event.clone())
                        .await
                    {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

static REGISTRY: Lazy<FeatureRegistry> = Lazy::new(FeatureRegistry::new);

pub fn registry() -> &'static FeatureRegistry {
    &REGISTRY
}
