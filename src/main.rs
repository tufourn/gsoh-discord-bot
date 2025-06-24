use anyhow::Context as AnyhowContext;
use poise::CreateReply;
use poise::serenity_prelude::{self as serenity, Attachment, ChannelType, GetMessages, MessageId};
use std::path::{Path, PathBuf};
use tracing::instrument;
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};
use validator::ValidateUrl;

struct Data {
    move_list: Vec<&'static str>,
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

const ALLOWED_CONTENT_TYPES: [&str; 2] = ["video/quicktime", "video/mp4"];
const FILE_UPLOAD_URL: &str = "https://0x0.st";
const MAX_TOTAL_SIZE_BYTES: u64 = 512 * 1024 * 1024; // 512MB
const USER_AGENT: &str = "GsohDiscordBot/1.0 (https://github.com/tufourn/gsoh-discord-bot)";

#[poise::command(slash_command)]
#[instrument(name = "pull", skip_all, fields(id = ctx.id(), username = ctx.author().name, move_name = move_name))]
async fn pull(
    ctx: Context<'_>,
    #[description = "Move name"] move_name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral()
        .await
        .context("Failed to defer response")?;

    let guild_channel = match ctx.guild_channel().await {
        Some(gc) => match gc.kind {
            ChannelType::NewsThread | ChannelType::PublicThread | ChannelType::PrivateThread => gc,
            _ => {
                ctx.send(CreateReply {
                    content: Some("This command must be run in a thread".to_owned()),
                    ephemeral: Some(true),
                    ..Default::default()
                })
                .await
                .context("Failed to send message")?;
                return Ok(());
            }
        },
        None => {
            ctx.send(CreateReply {
                content: Some("This command must be run in a thread".to_owned()),
                ephemeral: Some(true),
                ..Default::default()
            })
            .await
            .context("Failed to send message")?;
            return Ok(());
        }
    };

    if !ctx.data().move_list.contains(&move_name.as_str()) {
        ctx.send(CreateReply {
            content: Some(
                "Move not found, use `/search <page_number>` to get the move name".to_owned(),
            ),
            ephemeral: Some(true),
            ..Default::default()
        })
        .await
        .context("Failed to send message")?;
        return Ok(());
    }

    struct Submission {
        attachment: Attachment,
        username: String,
    }
    let mut submissions: Vec<Submission> = Vec::new();

    let mut last_message_id: Option<MessageId> = None;
    loop {
        let mut builder = GetMessages::new().limit(100);
        if let Some(id) = last_message_id {
            builder = builder.before(id);
        }

        let messages = guild_channel
            .id
            .messages(&ctx, builder)
            .await
            .context("Failed to retrieve messages")?;
        if messages.is_empty() {
            break;
        }

        last_message_id = messages.last().map(|m| m.id);
        for message in messages {
            for attachment in message.attachments {
                if attachment
                    .content_type
                    .as_deref()
                    .is_some_and(|ct| ALLOWED_CONTENT_TYPES.contains(&ct))
                {
                    submissions.push(Submission {
                        attachment,
                        username: message.author.name.to_owned(),
                    });
                }
            }
        }
    }

    if submissions.is_empty() {
        ctx.send(CreateReply {
            content: Some("No video (.mov or .mp4) found".to_owned()),
            ephemeral: Some(true),
            ..Default::default()
        })
        .await
        .context("Failed to send message")?;
        return Ok(());
    }

    let dir = tempfile::tempdir().context("Failed to create temporary directory")?;
    let zip_file_name = format!("{}.zip", &move_name);
    let zip_file_path = dir.path().join(&zip_file_name);

    struct ArchiveResult {
        archive: PathBuf,
        message: Option<String>,
    }

    let archive_result = tokio::task::spawn_blocking(move || {
        let zip_file = std::fs::File::create(&zip_file_path).context("Failed to create archive")?;
        let mut zip = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let mut total_size = 0;
        let mut message = None;

        for submission in submissions {
            let file_extension = match Path::new(&submission.attachment.filename)
                .extension()
                .and_then(std::ffi::OsStr::to_str)
            {
                Some(ext) => ext,
                None => continue,
            };

            if total_size + submission.attachment.size as u64 > MAX_TOTAL_SIZE_BYTES {
                message = Some(format!(
                    "Size limit 512MB reached. Messages from {} and earlier were not downloaded",
                    submission.attachment.id.created_at()
                ));
                break;
            }
            total_size += submission.attachment.size as u64;

            let new_file_name = format!(
                "{}-{}-{}.{}",
                &move_name, &submission.username, submission.attachment.id, file_extension
            );

            let mut response = reqwest::blocking::get(&submission.attachment.url)
                .context("Failed to get attachment")?;
            zip.start_file(&new_file_name, options).context(format!(
                "Failed to start writing attachment {}",
                submission.attachment.id,
            ))?;
            std::io::copy(&mut response, &mut zip).context(format!(
                "Failed to write attachment {}",
                submission.attachment.id
            ))?;
        }

        zip.finish()
            .context("Failed to finish writing to archive")?;

        Ok::<ArchiveResult, Error>(ArchiveResult {
            archive: zip_file_path,
            message,
        })
    })
    .await
    .context("Failed to create archive")??;

    if archive_result.message.is_some() {
        ctx.send(CreateReply {
            content: archive_result.message,
            ephemeral: Some(true),
            ..Default::default()
        })
        .await
        .context("Failed to send message")?;
    }

    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let form = reqwest::multipart::Form::new()
        .text("expires", "1") // download link expires in 1 hour
        .file("file", archive_result.archive)
        .await
        .context("Failed to create upload form")?;

    let response = client
        .post(FILE_UPLOAD_URL)
        .multipart(form)
        .send()
        .await
        .context("Failed to send request")?
        .text()
        .await
        .context("Failed to get response text")?;

    let reply = if response.validate_url() {
        // 0x0.st renames the uploaded file
        // append zip filename to download url to get correct filename
        format!(
            "{}/{}\nLink expires in 1 hour",
            &response.trim(),
            zip_file_name
        )
    } else {
        tracing::error!("Failed to create download link. Response:\n{}", response);
        "Failed to create download link".to_string()
    };

    ctx.send(CreateReply {
        content: Some(reply),
        ephemeral: Some(true),
        ..Default::default()
    })
    .await
    .context("Failed to send message")?;

    dir.close()
        .context("Failed to close and remove temporary directory")?;

    Ok(())
}

#[poise::command(slash_command)]
#[instrument(name = "search", skip_all, fields(id = ctx.id(), username = ctx.author().name, search_term = search_term))]
async fn search(
    ctx: Context<'_>,
    #[description = "Search term"] search_term: String,
) -> Result<(), Error> {
    let search_term = search_term.to_lowercase();

    let results: Vec<&'static str> = ctx
        .data()
        .move_list
        .iter()
        .filter(|line| line.contains(&search_term))
        .cloned()
        .collect();

    let reply = if results.is_empty() {
        format!("No move contains {}", search_term)
    } else {
        format!(
            "Moves containing \"{}\":\n{}",
            search_term,
            results.join("\n")
        )
    };

    ctx.send(CreateReply {
        content: Some(reply),
        ephemeral: Some(true),
        ..Default::default()
    })
    .await
    .context("Failed to send message")?;

    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,serenity=error".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_span_events(FmtSpan::NEW | FmtSpan::CLOSE))
        .init();

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN not set");
    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;

    let move_list: Vec<&'static str> = include_str!("../move-list.txt").lines().collect();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![pull(), search()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data { move_list })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;

    client.unwrap().start().await.unwrap();
}
