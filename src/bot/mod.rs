pub mod discord;

use anyhow::Result;

pub async fn run() -> Result<()> {
    discord::run().await
}
