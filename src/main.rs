//! StreamBridge - Entry Point
//! ==========================
//! This is the main entry point for StreamBridge. It is responsible for:
//!
//! 1. Loading configuration from config.toml
//! 2. Verifying the Discord bot token is valid
//! 3. Establishing a connection to the Discord Gateway
//!
//! If you are new to Rust, the `main` function is where every Rust program starts execution, similar to `main` in C, C++, or Java.
//!
//! For more information on how StreamBridge works, see the docs/ directory.
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::fs;
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// The Discord API version this bot is built against.
/// If Discord releases a new API version, this is the only place that needs updating.
/// Discord API versioning: https://docs.discord.com/developers/reference#api-versioning
const DISCORD_API_VERSION: u8 = 10;

/// The base URL for all Discord REST API requests.
const DISCORD_API_BASE: &str = "https://discord.com/api";

/// The base URL for the Discord WebSocket gateway.
const DISCORD_GATEWAY_BASE: &str = "wss://gateway.discord.gg";

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

/// Represents a raw Gateway event payload received from Discord.
/// Every message from the Gateway follows this structure regardless of type.
/// Gateway payload structure: https://docs.discord.com/developers/events/gateway-events#payload-structure
#[derive(Deserialize)]
struct GatewayPayload {
    /// The opcode indicating what type of event this is.
    /// Full list of opcodes: https://docs.discord.com/developers/topics/opcodes-and-status-codes#gateway-gateway-opcodes
    op: u8,

    /// The event data. Its structure depends on the opcode.
    d: Option<serde_json::Value>,

    /// The sequence number. Only present for opcode 0 (Dispatch) events.
    /// Must be cached and sent with heartbeats.
    s: Option<u64>,

    /// The event name. Only present for opcode 0 (Dispatch) events.
    t: Option<String>,
}

/// Establishes a connection to the Discord Gateway and performs the initial handshake sequence:
/// 1. Connect to the Gateway WebSocket URL
/// 2. Receive Hello (opcode 10) and extract the heartbeat interval
/// 3. Send Identify (opcode 2) with our bot token and intents
/// 4. Receive Ready (opcode 0) confirming the connection is established
///
/// Gateway connection documentation:
/// https://docs.discord.com/developers/events/gateway#connections
async fn connect_gateway(token: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Build the gateway URL with version and encoding query parameters.
    // We explicitly request JSON encoding and API v10.
    // Gateway URL documentation:
    // https://docs.discord.com/developers/events/gateway#connecting
    let gateway_url = format!(
        "{}/?v={}&encoding=json",
        DISCORD_GATEWAY_BASE, DISCORD_API_VERSION
    );

    println!("Connecting to Discord Gateway at {}", gateway_url);

    // Open the WebSocket connection to Discord's Gateway.
    // connect_async performs the WebSocket handshake over TLS automatically.
    let (mut ws_stream, _) = connect_async(&gateway_url).await?;

    println!("WebSocket connection established");

    // The first message Discord sends after connecting is always Hello (opcode 10).
    // It contains the heartbeat_interval we need to maintain the connection.
    // Hello event documentation:
    // https://docs.discord.com/developers/events/gateway#hello-event
    let hello_msg = ws_stream
        .next()
        .await
        .ok_or("Gateway closed before sending Hello")??;

    // Parse the raw WebSocket message as a GatewayPayload
    let hello_payload: GatewayPayload = serde_json::from_str(hello_msg.to_text()?)?;

    // Verify we actually received a Hello (opcode 10) and not something unexpected
    if hello_payload.op != 10 {
        return Err(format!(
            "Expected Hello (opcode 10), got opcode {}",
            hello_payload.op
        )
        .into());
    }

    // Extract the heartbeat_interval from the Hello payload's data field.
    // This tells us how often we need to send heartbeats to keep the connection alive.
    let heartbeat_interval = hello_payload
        .d
        .as_ref()
        .and_then(|d| d.get("heartbeat_interval"))
        .and_then(|v| v.as_u64())
        .ok_or("Hello payload missing heartbeat_interval")?;

    println!("Heartbeat interval: {}ms", heartbeat_interval);

    // Send the Identify payload to perform the initial handshake.
    // This tells Discord who we are, what events we want, and what platform we're on.
    // Identify documentation:
    // https://docs.discord.com/developers/events/gateway#identifying
    //
    // Intents are bitwise values that tell Discord which events to send us.
    // We are requesting:
    // - GUILDS (1 << 0) = 1
    // - GUILD_MESSAGES (1 << 9) = 512
    // - MESSAGE_CONTENT (1 << 15) = 32768
    // Total: 1 | 512 | 32768 = 33281
    // Intents documentation:
    // https://docs.discord.com/developers/events/gateway#gateway-intents
    let identify_payload = serde_json::json!({
        "op": 2,
        "d": {
            "token": token,
            "intents": 33281,
            "properties": {
                "os": std::env::consts::OS,
                "browser": "StreamBridge",
                "device": "StreamBridge"
            }
        }
    });

    ws_stream
        .send(Message::Text(identify_payload.to_string().into()))
        .await?;

    println!("Identify sent");

    // Wait for the Ready event (opcode 0, event name "READY") which confirms our Identify was accepted and we are connected to the Gateway.
    // Ready event documentation:
    // https://docs.discord.com/developers/events/gateway#ready-event
    let ready_msg = ws_stream
        .next()
        .await
        .ok_or("Gateway closed before sending Ready")??;

    let ready_payload: GatewayPayload = serde_json::from_str(ready_msg.to_text()?)?;

    if ready_payload.op == 0 && ready_payload.t.as_deref() == Some("READY") {
        println!("Gateway connection established - StreamBridge is ready!");
    } else {
        return Err(format!(
            "Expected Ready event, got op={} t={:?}",
            ready_payload.op, ready_payload.t
        )
        .into());
    }

    Ok(())
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
        .get(format!(
            "{}/v{}/users/@me",
            DISCORD_API_BASE, DISCORD_API_VERSION
        ))
        .header("Authorization", format!("Bot {}", config.discord.token))
        .send()
        .await?;

    let body = response.text().await?;
    println!("{}", body);

    // Connect to the Discord Gateway
    connect_gateway(&config.discord.token).await?;

    Ok(())
}
