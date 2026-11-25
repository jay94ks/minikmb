//! Manual browser-test host: temporary profile, no discovery or connected devices.
use std::{io::Write, time::Duration};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let app = minikmb_native::service::AppService::new(directory.path().into())?;
    let key = app.keys.create("temporary browser fixture")?;
    let servers = minikmb_native::remote::RemoteServers::new(app.clone());
    servers.rest(true, 0).await?;
    println!(
        "{}",
        serde_json::json!({"address":format!("http://{}",app.remote.read().rest.unwrap()),"token":key.token.expose()})
    );
    std::io::stdout().flush()?;
    // Explicit fixture execution only. The host expires even if its caller exits.
    tokio::time::sleep(Duration::from_secs(300)).await;
    servers.shutdown().await;
    app.shutdown().await;
    Ok(())
}
