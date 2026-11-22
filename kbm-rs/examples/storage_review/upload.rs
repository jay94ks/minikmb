use minikmb_native::{remote::RemoteServers, service::AppService};
use serde_json::{json, Value};
use std::{
    io::{Seek, SeekFrom, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const SIZE: u64 = 10_000_000u64.div_ceil(2048) * 2048;

// Transfer fixture only: ISO/El Torito headers accepted by the limited profile,
// not an OS boot acceptance test. No sparse or zero-tail transfer shortcut.
fn fixture() -> std::io::Result<(tempfile::NamedTempFile, u32)> {
    let mut file = tempfile::NamedTempFile::new()?;
    let mut prefix = vec![0; 40 * 2048];
    let p = &mut prefix[16 * 2048..17 * 2048];
    p[0] = 1;
    p[1..6].copy_from_slice(b"CD001");
    p[6] = 1;
    p[80..84].copy_from_slice(&((SIZE / 2048) as u32).to_le_bytes());
    p[84..88].copy_from_slice(&((SIZE / 2048) as u32).to_be_bytes());
    p[128..130].copy_from_slice(&2048u16.to_le_bytes());
    p[130..132].copy_from_slice(&2048u16.to_be_bytes());
    let p = &mut prefix[17 * 2048..18 * 2048];
    p[1..6].copy_from_slice(b"CD001");
    p[6] = 1;
    p[7..30].copy_from_slice(b"EL TORITO SPECIFICATION");
    p[71..75].copy_from_slice(&20u32.to_le_bytes());
    let p = &mut prefix[18 * 2048..19 * 2048];
    p[0] = 255;
    p[1..6].copy_from_slice(b"CD001");
    p[6] = 1;
    let p = &mut prefix[20 * 2048..21 * 2048];
    p[0] = 1;
    p[30..32].copy_from_slice(&[0x55, 0xaa]);
    p[28..30].copy_from_slice(&0x55aau16.to_le_bytes());
    p[32] = 0x88;
    p[38..40].copy_from_slice(&1u16.to_le_bytes());
    p[40..44].copy_from_slice(&21u32.to_le_bytes());
    let mut crc = crc32fast::Hasher::new();
    file.write_all(&prefix)?;
    crc.update(&prefix);
    let mut offset = prefix.len() as u64;
    let mut state = 0x6d2b_79f5u32;
    let mut block = [0u8; 64 * 1024];
    while offset < SIZE {
        let count = (SIZE - offset).min(block.len() as u64) as usize;
        for b in &mut block[..count] {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *b = state as u8;
        }
        file.write_all(&block[..count])?;
        crc.update(&block[..count]);
        offset += count as u64;
    }
    file.flush()?;
    file.seek(SeekFrom::Start(0))?;
    Ok((file, crc.finalize()))
}

// curl is a test driver only; production transports remain Rust axum/rmcp.
// The disposable key is never printed or included in the acceptance report.
async fn post(
    url: String,
    token: &str,
    body: &Value,
    file: Option<&Path>,
) -> Result<Value, String> {
    let mut command = std::process::Command::new("curl.exe");
    command
        .args([
            "--silent",
            "--show-error",
            "--fail-with-body",
            "--max-time",
            "30",
            "--noproxy",
            "*",
            "-X",
            "POST",
            "-H",
        ])
        .arg(format!("Authorization: Bearer {token}"));
    if let Some(file) = file {
        command
            .args([
                "-H",
                "Content-Type: application/octet-stream",
                "--data-binary",
            ])
            .arg(format!("@{}", file.display()));
    } else {
        command
            .args(["-H", "Content-Type: application/json", "--data-binary"])
            .arg(body.to_string());
    }
    command.arg(url);
    let output = tokio::task::spawn_blocking(move || command.output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "REST response was not JSON".to_owned())?;
    if !output.status.success() {
        return Err(format!("REST request failed: {value}"));
    }
    Ok(value)
}

fn require_empty_card(snapshot: &Value) -> Result<(), String> {
    if snapshot["images"].as_array().is_none_or(|a| !a.is_empty())
        || !snapshot["optical"].is_null()
        || !snapshot["disk"].is_null()
        || snapshot["card"]["present"] != true
        || snapshot["card"]["initialized"] != true
        || snapshot["stale"] != false
        || snapshot["card"]["current_uid"] != snapshot["uid"]
        || snapshot["card"]["stored_uid"] != snapshot["uid"]
        || snapshot["card"]["uuid"]
            .as_str()
            .is_none_or(|s| s.is_empty())
    {
        return Err("write acceptance requires an initialized card and an empty image catalog; existing media is preserved".into());
    }
    Ok(())
}

pub async fn run(
    app: &Arc<AppService>,
    servers: &Arc<RemoteServers>,
    before: &Value,
) -> Result<Value, String> {
    let snapshot = &before["snapshot"];
    require_empty_card(snapshot)?;
    let (source, crc) = tokio::task::spawn_blocking(fixture)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let key = app
        .keys
        .create("temporary storage acceptance")
        .map_err(|e| e.to_string())?;
    servers.rest(true, 0).await.map_err(|e| e.to_string())?;
    let address = app.remote.read().rest.ok_or("REST did not start")?;
    let base = format!("http://{address}");
    let token = key.token.expose();
    let spec = json!({"kind":"optical", "name":"native-10mb-transfer.iso", "size":SIZE, "confirmed":true,
        "expected_card_uuid":snapshot["card"]["uuid"]});
    let ticket = post(
        format!("{base}/api/v1/active/storage/uploads/prepare"),
        token,
        &spec,
        None,
    )
    .await?;
    let url = ticket["url"].as_str().ok_or("upload ticket has no URL")?;
    if !url.starts_with(&format!("{base}/api/v1/active/storage/uploads/")) {
        return Err("unexpected upload origin".into());
    }
    let started = Instant::now();
    let accepted = post(url.to_owned(), token, &json!({}), Some(source.path())).await?;
    let id = accepted["job_id"]
        .as_str()
        .ok_or("upload did not return a job ID")?;
    let mut last = Instant::now();
    loop {
        let job = post(
            format!("{base}/api/v1/storage/jobs/get"),
            token,
            &json!({"job_id":id}),
            None,
        )
        .await?;
        if last.elapsed() >= Duration::from_secs(10) {
            println!(
                "phase={} done={} total={} elapsed={:.1}s",
                job["phase"],
                job["done"],
                job["total"],
                started.elapsed().as_secs_f64()
            );
            last = Instant::now();
        }
        match job["state"].as_str() {
            Some("completed") => {
                if job["result"]["image"]["size"] != SIZE
                    || job["result"]["image"]["crc32"] != crc
                    || job["result"]["snapshot"]["optical"]["size"] != SIZE
                    || job["result"]["snapshot"]["optical"]["crc"] != crc
                    || job["result"]["snapshot"]["optical"]["mounted"] != false
                {
                    return Err(
                        "completed SD image size/CRC/unmounted state differs from source".into(),
                    );
                }
                return Ok(
                    json!({"mode":"10mb-rest-upload", "bytes":SIZE,"source_crc32":crc,"job":job,
                    "elapsed_seconds":started.elapsed().as_secs_f64(), "verification":"MCU full SD file reread and CRC32 check at COMMIT; matching native final metadata",
                    "host_full_readback":false, "existing_catalog_empty":true, "target_mount_requested":false}),
                );
            }
            Some("failed" | "cancelled") => return Err(format!("SD upload failed: {job}")),
            Some("queued" | "running") => {}
            _ => return Err("unexpected SD job state".into()),
        }
        if started.elapsed() > Duration::from_secs(300) {
            return Err("10MB transfer exceeded 5 minutes".into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn fixture_is_ten_mb_aligned_profile_with_full_file_checksum() {
        let (mut file, expected) = fixture().unwrap();
        assert_eq!(file.as_file().metadata().unwrap().len(), 10_000_384);
        minikmb_native::storage::optical::inspect(SIZE, |lba| {
            let mut sector = [0; 2048];
            file.seek(SeekFrom::Start(lba * 2048)).unwrap();
            file.read_exact(&mut sector).unwrap();
            Ok(sector)
        })
        .unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut checksum = crc32fast::Hasher::new();
        let mut buffer = [0; 65536];
        let mut bytes = 0;
        loop {
            let read = file.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            checksum.update(&buffer[..read]);
            bytes += read as u64;
        }
        assert_eq!(bytes, SIZE);
        assert_eq!(checksum.finalize(), expected);
    }

    #[test]
    fn existing_media_stale_or_wrong_card_cannot_start_a_write_review() {
        let valid = json!({"uid":"fixture", "stale":false, "images":[], "optical":null,"disk":null,
            "card":{"present":true,"initialized":true,"current_uid":"fixture","stored_uid":"fixture","uuid":"fixture-card"}});
        assert!(require_empty_card(&valid).is_ok());
        for (field, value) in [
            ("images", json!([{"id":1}])),
            ("optical", json!({"id":1})),
            ("disk", json!({"lease":2})),
            ("stale", json!(true)),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(require_empty_card(&invalid).is_err(), "{field}");
        }
        for (field, value) in [
            ("present", json!(false)),
            ("initialized", json!(false)),
            ("current_uid", json!("other")),
            ("stored_uid", json!("other")),
            ("uuid", json!("")),
        ] {
            let mut invalid = valid.clone();
            invalid["card"][field] = value;
            assert!(require_empty_card(&invalid).is_err(), "card/{field}");
        }
    }
}
