//! Native storage acceptance driver, outside automatic tests. Default is read-only.
//! The explicit write option also requires an empty catalog; it never formats,
//! replaces, mounts or deletes an existing card/image. No arbitrary COM fallback.
#[path = "storage_review/upload.rs"]
mod upload;
use minikmb_native::{
    input_sessions::Principal,
    model::Uid,
    service::AppService,
    storage::{Action, JobInfo, JobState},
};
use serde_json::json;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

async fn finish(app: &Arc<AppService>, owner: &Principal, id: &str) -> Result<JobInfo, String> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let info = app.storage.job(owner, id).map_err(|e| e.to_string())?;
            if !matches!(info.state, JobState::Queued | JobState::Running) {
                return Ok(info);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| "storage preflight timed out".to_owned())?
}

async fn preflight(app: &Arc<AppService>, uid: Uid) -> Result<serde_json::Value, String> {
    let devices = app.devices.clone();
    tokio::task::spawn_blocking(move || devices.scan())
        .await
        .map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let inventory = app.devices.inventory();
        let ready = inventory
            .channels
            .iter()
            .any(|c| c.uid == uid && c.connected);
        *app.inventory.write() = inventory;
        if ready {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("the exact allowlisted management UID did not connect".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    app.select(Some(uid)).await.map_err(|e| e.to_string())?;
    let owner = Principal {
        key_id: "local-storage-review".into(),
        client_id: "read-only-preflight".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    };
    let job = app
        .storage
        .start(
            app.active.bind().map_err(|e| e.to_string())?,
            owner.clone(),
            Action::Refresh {},
        )
        .map_err(|e| e.to_string())?;
    let job = finish(app, &owner, &job.job_id).await?;
    Ok(json!({"mode":"read-only-preflight", "job":job,
        "snapshot":app.storage.cached(&app.active.bind().map_err(|e| e.to_string())?),
        "card_or_image_writes":false}))
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !(args.len() == 3 || (args.len() == 4 && args[3] == "--upload-10mb")) {
        return Err(
            "usage: storage_review EXACT_UID ABSOLUTE_NEW_REPORT_JSON [--upload-10mb]".into(),
        );
    }
    let uid = Uid::try_from(args[1].clone())?;
    let output = PathBuf::from(&args[2]);
    if !output.is_absolute() || output.exists() || !output.parent().is_some_and(|p| p.is_dir()) {
        return Err("report must be a new absolute file in an existing directory".into());
    }
    let profile = tempfile::tempdir()?;
    let app = AppService::new(profile.path().to_owned())?;
    app.logs.quiet();
    let servers = minikmb_native::remote::RemoteServers::new(app.clone());
    let write = args.len() == 4;
    let result = match preflight(&app, uid).await {
        Ok(before) if write && before["job"]["state"] == "completed" => {
            upload::run(&app, &servers, &before).await
        }
        result => result,
    };
    servers.shutdown().await;
    app.shutdown().await;
    let report = match result {
        Ok(v) => v,
        Err(error) => {
            json!({"mode":if write {"10mb-rest-upload"} else {"read-only-preflight"}, "error":error, "card_or_image_writes_possible":write})
        }
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(file, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    if report.get("error").is_some() || report["job"]["state"] != "completed" {
        return Err("storage acceptance did not complete; see report".into());
    }
    Ok(())
}
