## GSOH Discord Bot

Makes it easier to download videos from threads in the Gambling Sleight of Hand (GSOH) Discord server.

This bot uses [`serenity`](https://crates.io/crates/serenity) and [`poise`](https://crates.io/crates/poise) to download videos from threads, uses [`zip`](https://crates.io/crates/zip) to compress them into an archive, and then uploads the archive via [`reqwest`](https://crates.io/crates/reqwest) to [`0x0.st`](https://0x0.st) to obtain a temporary download link.

The download link expires in 1 hour to avoid overloading the file hosting server.

### Usage

| Command | Description |
| :------------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/pull <move_name>` | Zips all `.mov` or `.mp4` video attachments from the **current thread** with total size limit of **512MB**. Archive is named `<move_name>.zip`. Each file inside is named `<move_name>-<author_username>-<attachment_id>.<extension>`. |
| `/search <search_term>` | Searches the bot's move list (stored in `move-list.txt`) for finding the exact `move_name` to use with the `/pull` command. Using the page number as a `search_term` often yields the best results. |

#### Example usage
In the `Conley Three-Riffle Variation (Page 107)` thread, do `/search 107`. The bot responds with:
```
Moves containing "107":
02-false_shuffles-0107-conleys_three_riffle_variation
```

Then run 

```
/pull 02-false_shuffles-0107-conleys_three_riffle_variation
```

to download the videos in the thread.

### Setup Instructions

If you want to run the bot yourself:

1.  **Prerequisites**: Ensure you have [Rust](https://www.rust-lang.org/tools/install) installed.
2.  **Clone the repository**:
    ```bash
    git clone https://github.com/tufourn/gsoh-discord-bot
    cd gsoh-discord-bot
    ```
3.  **Create your Discord Bot**:
    * Go to the [Discord Developer Portal](https://discord.com/developers/applications).
    * Create a new application and then create a bot user.
    * Under the "Bot" tab, enable the **Message Content Intent** and ensure the bot has **Read Message History** and **Send Message** permissions.
    * Copy your bot's token.
5.  **Set up Environment Variables**: Create a `.env` file in the project root with your Discord bot token:
    ```
    DISCORD_TOKEN=YOUR_DISCORD_BOT_TOKEN_HERE
    ```
    Replace `YOUR_DISCORD_BOT_TOKEN_HERE` with the actual token you copied from your Discord Developer Portal.
6.  **Run the bot**:
    ```bash
    cargo run
    ```
    The bot should now connect to Discord. Invite it to your server and ensure it has the necessary permissions.
