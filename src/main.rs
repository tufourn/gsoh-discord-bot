use curl::easy::{Easy, Form};
use poise::CreateReply;
use poise::serenity_prelude::{
    self as serenity, Attachment, AttachmentId, GetMessages, MessageId, User,
};
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tempfile::tempdir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

struct Data {
    move_list: Vec<&'static str>,
    curl_handle: Arc<tokio::sync::Mutex<Easy>>,
}
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

const ALLOWED_CONTENT_TYPE: [&str; 2] = ["video/quicktime", "video/mp4"];

#[poise::command(slash_command)]
async fn pull(
    ctx: Context<'_>,
    #[description = "Move name"] move_name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let guild_channel = match ctx.guild_channel().await {
        Some(gc) => match gc.kind {
            serenity::ChannelType::NewsThread
            | serenity::ChannelType::PublicThread
            | serenity::ChannelType::PrivateThread => gc,
            _ => {
                ctx.send(CreateReply {
                    content: Some("This command must be run in a thread".to_owned()),
                    ephemeral: Some(true),
                    ..Default::default()
                })
                .await?;
                return Ok(());
            }
        },
        None => {
            ctx.send(CreateReply {
                content: Some("This command must be run in a thread".to_owned()),
                ephemeral: Some(true),
                ..Default::default()
            })
            .await?;
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
        .await?;
        return Ok(());
    }

    let mut all_messages = Vec::new();
    let mut last_message_id: Option<MessageId> = None;

    loop {
        let mut builder = GetMessages::new().limit(100);
        if let Some(id) = last_message_id {
            builder = builder.before(id);
        }

        let messages = guild_channel.id.messages(&ctx, builder).await?;
        if messages.is_empty() {
            break;
        }

        last_message_id = messages.last().map(|m| m.id);
        all_messages.extend(messages);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    struct Submission {
        id: AttachmentId,
        attachment: Attachment,
        content: Vec<u8>,
        user: User,
    }

    let mut submissions: Vec<Submission> = Vec::new();
    for message in all_messages {
        for attachment in message.attachments {
            if attachment
                .content_type
                .as_deref()
                .is_some_and(|ct| ALLOWED_CONTENT_TYPE.contains(&ct))
            {
                let content = match attachment.download().await {
                    Ok(content) => content,
                    Err(e) => {
                        ctx.send(CreateReply {
                            content: Some(format!("Error downloading attachment: {:?}", e)),
                            ephemeral: Some(true),
                            ..Default::default()
                        })
                        .await?;
                        continue;
                    }
                };

                submissions.push(Submission {
                    id: attachment.id,
                    attachment,
                    content,
                    user: message.author.clone(),
                });
            }
        }
    }

    if submissions.is_empty() {
        ctx.send(CreateReply {
            content: Some("No video (.mov or .mp4) found".to_owned()),
            ephemeral: Some(true),
            ..Default::default()
        })
        .await?;
        return Ok(());
    }

    let curl_handle_arc = ctx.data().curl_handle.clone();
    let move_name_clone = move_name.clone();

    let zip_and_upload: Result<String, Error> = tokio::task::spawn_blocking(move || {
        let dir = tempdir()?;
        let zip_file_path = dir.path().join(format!("{}.zip", &move_name_clone));
        let zip_file = std::fs::File::create(&zip_file_path)?;
        let mut zip = ZipWriter::new(zip_file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        for submission in submissions {
            let file_extension = match Path::new(&submission.attachment.filename)
                .extension()
                .and_then(OsStr::to_str)
            {
                Some(ext) => ext,
                None => continue,
            };
            let new_file_name = format!(
                "{}-{}-{}.{}",
                &move_name_clone, submission.user.name, submission.id, file_extension
            );

            zip.start_file(&new_file_name, options)?;
            zip.write_all(&submission.content)?;
        }

        zip.finish()?;

        let mut easy_handle = curl_handle_arc.blocking_lock();
        easy_handle.reset();
        let mut form = Form::new();
        form.part("file").file(&zip_file_path).add()?;
        form.part("expires").contents(b"1").add()?;
        easy_handle.url("https://0x0.st")?;
        easy_handle.useragent("curl/7.54.1")?;
        easy_handle.httppost(form)?;
        let mut response_body = Vec::new();
        {
            let mut transfer = easy_handle.transfer();
            transfer.write_function(|data| {
                response_body.extend_from_slice(data);
                Ok(data.len())
            })?;
            transfer.perform()?;
        }

        let response_str = String::from_utf8(response_body)?;

        dir.close()?;
        Ok(response_str)
    })
    .await?;

    let response = match zip_and_upload {
        Ok(download_link) => format!(
            "{}/{}.zip\nLink expires in 1 hour",
            &download_link.trim(),
            &move_name
        ),
        Err(_) => "Failed to generate download link".to_owned(),
    };
    ctx.send(CreateReply {
        content: Some(response),
        ephemeral: Some(true),
        ..Default::default()
    })
    .await?;

    Ok(())
}

#[poise::command(slash_command)]
async fn search(
    ctx: Context<'_>,
    #[description = "Search term"] search_term: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let search_term = search_term.to_lowercase();

    let results: Vec<&'static str> = ctx
        .data()
        .move_list
        .iter()
        .filter(|line| line.contains(&search_term))
        .cloned()
        .collect();

    let response = if results.is_empty() {
        format!("No move contains {}", search_term)
    } else {
        format!(
            "Moves containing \"{}\":\n{}",
            search_term,
            results.join("\n")
        )
    };

    ctx.send(CreateReply {
        content: Some(response),
        ephemeral: Some(true),
        ..Default::default()
    })
    .await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN not set");
    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;

    let move_list: Vec<&'static str> = include_str!("../move-list.txt").lines().collect();
    let curl_handle = Arc::new(tokio::sync::Mutex::new(Easy::new()));

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![pull(), search()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(Data {
                    move_list,
                    curl_handle,
                })
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await;
    client.unwrap().start().await.unwrap();
}
