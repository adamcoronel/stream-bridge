use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();

    let response = client
        .get("https://discord.com/api/v10/gateway")
        .send()
        .await?;

    let body = response.text().await?;

    println!("{}", body);

    Ok(())
}
