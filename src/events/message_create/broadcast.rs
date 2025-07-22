use crate::{
    context::Context,
    services::{broadcast::BroadcastService, channel::ChannelService},
};
use std::sync::Arc;
use twilight_model::channel::Message;

pub async fn handle_broadcast(ctx: &Arc<Context>, message: &Message) {
    for channel in ChannelService::get(ctx, message.channel_id.get()).await {
        if channel.channel_type.is_broadcast() {
            BroadcastService::handle(ctx, message, channel.channel_type).await;
        }
    }
}
