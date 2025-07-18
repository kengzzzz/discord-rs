use crate::{
    context::Context,
    dbs::mongo::models::channel::ChannelEnum,
    services::{broadcast::BroadcastService, channel::ChannelService},
};
use std::sync::Arc;
use twilight_model::channel::Message;

pub async fn handle_broadcast(ctx: Arc<Context>, message: &Message) {
    for channel in ChannelService::get(&ctx, message.channel_id.get()).await {
        if channel.channel_type == ChannelEnum::Broadcast {
            BroadcastService::handle(ctx.clone(), message).await;
        }
    }
}
