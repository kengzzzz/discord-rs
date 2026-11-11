use twilight_model::{
    application::{
        command::{CommandOptionChoice, CommandOptionChoiceValue},
        interaction::Interaction,
    },
    channel::{
        Message,
        message::{AllowedMentions, Component, MessageFlags},
    },
    http::interaction::{InteractionResponse, InteractionResponseData, InteractionResponseType},
};

use std::time::Duration;

use twilight_http::request::{Request, TryIntoRequest as _};

use super::errors;
use crate::app::App;

fn flags(ephemeral: bool) -> MessageFlags {
    if ephemeral {
        MessageFlags::IS_COMPONENTS_V2 | MessageFlags::EPHEMERAL
    } else {
        MessageFlags::IS_COMPONENTS_V2
    }
}

const CALLBACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

async fn send(
    app: &App,
    interaction: &Interaction,
    response: InteractionResponse,
) -> anyhow::Result<()> {
    // Without a rate limiter twilight retries a 429 forever.
    tokio::time::timeout(
        CALLBACK_TIMEOUT,
        app.callbacks
            .interaction(interaction.application_id)
            .create_response(interaction.id, &interaction.token, &response),
    )
    .await
    .map_err(|_| anyhow::anyhow!("interaction callback timed out"))??;
    Ok(())
}

pub async fn respond(
    app: &App,
    interaction: &Interaction,
    components: Vec<Component>,
    ephemeral: bool,
) -> anyhow::Result<()> {
    send(
        app,
        interaction,
        InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(InteractionResponseData {
                components: Some(components),
                flags: Some(flags(ephemeral)),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            }),
        },
    )
    .await
}

pub async fn defer(app: &App, interaction: &Interaction, ephemeral: bool) -> anyhow::Result<()> {
    send(
        app,
        interaction,
        InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(InteractionResponseData {
                flags: ephemeral.then_some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        },
    )
    .await
}

pub async fn defer_update(app: &App, interaction: &Interaction) -> anyhow::Result<()> {
    send(
        app,
        interaction,
        InteractionResponse { kind: InteractionResponseType::DeferredUpdateMessage, data: None },
    )
    .await
}

pub async fn edit(
    app: &App,
    interaction: &Interaction,
    components: &[Component],
) -> anyhow::Result<Message> {
    webhook(
        app,
        app.http
            .interaction(interaction.application_id)
            .update_response(&interaction.token)
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(Some(components))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .try_into_request()?,
    )
    .await
}

pub async fn followup(
    app: &App,
    interaction: &Interaction,
    components: &[Component],
    ephemeral: bool,
) -> anyhow::Result<Message> {
    webhook(
        app,
        app.http
            .interaction(interaction.application_id)
            .create_followup(&interaction.token)
            .flags(flags(ephemeral))
            .components(components)
            .allowed_mentions(Some(&AllowedMentions::default()))
            .try_into_request()?,
    )
    .await
}

const DISCORD_API: &str = "https://discord.com/api/v10/";
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(15);
const WEBHOOK_BODY_MAX: usize = 1 << 20;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(5);
const DISCORD_USER_AGENT: &str =
    "DiscordBot (https://twilight.rs/chapter_1_crates/section_2_http.html, 0.18.0) Twilight-rs";

/// Skips twilight's rate limiter, which keeps state per interaction token (see vendor PATCH.md).
/// A 429 waits `retry_after` once; a second one is an error.
async fn webhook(app: &App, request: Request) -> anyhow::Result<Message> {
    let body = send_webhook(&app.web, DISCORD_API, &request).await?;
    Ok(serde_json::from_slice(&body)?)
}

const WEBHOOK_ERROR_DETAIL: usize = 500;

async fn send_webhook(
    web: &reqwest::Client,
    base: &str,
    request: &Request,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        request.form().is_none(),
        "files are not sent through interaction webhooks"
    );
    let url = format!("{base}{}", request.path());
    let method = reqwest::Method::from_bytes(request.method().name().as_bytes())?;
    let mut retried = false;
    loop {
        let mut builder = web
            .request(method.clone(), &url)
            .header(reqwest::header::USER_AGENT, DISCORD_USER_AGENT)
            .timeout(WEBHOOK_TIMEOUT);
        if let Some(body) = request.body() {
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        // The URL holds the interaction token, so keep it out of logged errors.
        let mut response = builder
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)?
        {
            anyhow::ensure!(
                body.len() + chunk.len() <= WEBHOOK_BODY_MAX,
                "Discord body over {WEBHOOK_BODY_MAX} bytes"
            );
            body.extend_from_slice(&chunk);
        }
        if status.is_success() {
            return Ok(body);
        }
        let error: Option<serde_json::Value> = serde_json::from_slice(&body).ok();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && !retried {
            let wait = error
                .as_ref()
                .and_then(|e| e.get("retry_after")?.as_f64())
                .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
                .filter(|wait| *wait <= MAX_RETRY_AFTER);
            if let Some(wait) = wait {
                retried = true;
                tokio::time::sleep(wait).await;
                continue;
            }
        }
        return Err(errors::WebhookError {
            status: status.as_u16(),
            code: error
                .as_ref()
                .and_then(|e| e.get("code")?.as_u64()),
            detail: String::from_utf8_lossy(&body[..body.len().min(WEBHOOK_ERROR_DETAIL)])
                .into_owned(),
        }
        .into());
    }
}

pub async fn update(
    app: &App,
    interaction: &Interaction,
    components: Vec<Component>,
) -> anyhow::Result<()> {
    send(
        app,
        interaction,
        InteractionResponse {
            kind: InteractionResponseType::UpdateMessage,
            data: Some(InteractionResponseData {
                allowed_mentions: Some(AllowedMentions::default()),
                components: Some(components),
                flags: Some(MessageFlags::IS_COMPONENTS_V2),
                ..Default::default()
            }),
        },
    )
    .await
}

pub async fn modal(
    app: &App,
    interaction: &Interaction,
    custom_id: &str,
    title: &str,
    components: Vec<Component>,
) -> anyhow::Result<()> {
    send(
        app,
        interaction,
        InteractionResponse {
            kind: InteractionResponseType::Modal,
            data: Some(InteractionResponseData {
                custom_id: Some(custom_id.to_owned()),
                title: Some(title.to_owned()),
                components: Some(components),
                ..Default::default()
            }),
        },
    )
    .await
}

pub async fn autocomplete(
    app: &App,
    interaction: &Interaction,
    names: Vec<String>,
) -> anyhow::Result<()> {
    let choices = names
        .into_iter()
        .take(25)
        .map(|name| {
            let value: String = name.chars().take(100).collect();
            CommandOptionChoice {
                name: value.clone(),
                name_localizations: None,
                value: CommandOptionChoiceValue::String(value),
            }
        })
        .collect();
    send(
        app,
        interaction,
        InteractionResponse {
            kind: InteractionResponseType::ApplicationCommandAutocompleteResult,
            data: Some(InteractionResponseData { choices: Some(choices), ..Default::default() }),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use twilight_http::request::Method;

    use super::*;

    async fn server(
        responses: Vec<(u16, &'static str)>,
    ) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let mut served = 0;
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await.unwrap();
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(reply.as_bytes())
                    .await
                    .unwrap();
                served += 1;
            }
            served
        });
        (base, handle)
    }

    fn request() -> Request {
        Request::builder(
            &twilight_http::routing::Route::UpdateInteractionOriginal {
                application_id: 1,
                interaction_token: "t",
            },
        )
        .body(b"{}".to_vec())
        .build()
        .unwrap()
    }

    #[tokio::test]
    async fn edits_and_followups_are_json_to_the_interaction_webhook() {
        let http = twilight_http::Client::new("t".into());
        let components = crate::discord::ui::error("x");
        let id = twilight_model::id::Id::new(9);
        let edit = http
            .interaction(id)
            .update_response("tok")
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .components(Some(&components))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .try_into_request()
            .unwrap();
        assert_eq!(edit.method(), Method::Patch);
        assert_eq!(edit.path(), "webhooks/9/tok/messages/@original");
        let followup = http
            .interaction(id)
            .create_followup("tok")
            .flags(flags(true))
            .components(&components)
            .allowed_mentions(Some(&AllowedMentions::default()))
            .try_into_request()
            .unwrap();
        assert_eq!(followup.method(), Method::Post);
        assert!(
            followup
                .path()
                .starts_with("webhooks/9/tok")
        );
        for request in [&edit, &followup] {
            assert!(request.form().is_none() && !request.use_authorization_token());
            let body: serde_json::Value = serde_json::from_slice(request.body().unwrap()).unwrap();
            assert!(body["components"].is_array() && body["allowed_mentions"].is_object());
        }
    }

    #[tokio::test]
    async fn a_webhook_waits_out_one_rate_limit_only() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let web = reqwest::Client::new();
        assert_eq!(request().method(), Method::Patch);

        let (base, served) = server(vec![
            (429, r#"{"retry_after": 0.05, "global": false}"#),
            (200, "{}"),
        ])
        .await;
        assert_eq!(
            send_webhook(&web, &base, &request())
                .await
                .unwrap(),
            b"{}"
        );
        assert_eq!(served.await.unwrap(), 2);

        let (base, served) = server(vec![
            (429, r#"{"retry_after": 0.05}"#),
            (429, r#"{"retry_after": 0.05}"#),
        ])
        .await;
        let error = send_webhook(&web, &base, &request())
            .await
            .unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<errors::WebhookError>()
                .unwrap()
                .status,
            429
        );
        assert_eq!(served.await.unwrap(), 2);

        let (base, _) = server(vec![(429, r#"{"retry_after": 60}"#)]).await;
        assert!(
            send_webhook(&web, &base, &request())
                .await
                .is_err()
        );

        let (base, _) = server(vec![(
            404,
            r#"{"message": "Unknown interaction", "code": 10062}"#,
        )])
        .await;
        let error = send_webhook(&web, &base, &request())
            .await
            .unwrap_err();
        assert_eq!(
            errors::code(&error),
            Some(errors::UNKNOWN_INTERACTION)
        );
        assert!(
            error
                .to_string()
                .contains("Unknown interaction")
        );
    }
}
