use serde::Deserialize;
use std::fs;

/// The top level configuration structure for StreamBridge.
/// This is deserialized directly from config.toml.
#[derive(Deserialize)]
struct Config {
    discord: DiscordConfig,
}

/// Discord specific configuration values.
#[derive(Deserialize)]
struct DiscordConfig {
    token: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read the raw config file contents from disk
    let config_contents = fs::read_to_string("config.toml")?;

    // Parse the raw TOML string into our Config struct
    let config: Config = toml::from_str(&config_contents)?;

    println!("Bot token loaded successfully");

    // Create a persistent HTTP client for all Discord API requests
    let client = reqwest::Client::new();

    // Hit the /users/@me endpoint to verify our token is valid.
    // This endpoint returns the bot's own user profile.
    // Discord API reference: https://docs.discord.com/developers/resources/user#get-current-user
    let response = client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bot {}", config.discord.token))
        .send()
        .await?;

    let body = response.text().await?;

    println!("{}", body);

    Ok(())
}
