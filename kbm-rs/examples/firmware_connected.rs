//! Explicit single-STM32 review. --execute is never used by cargo tests.
use minikmb_native::{
    firmware::Target, input_sessions::Principal, model::Uid, service::AppService,
};
use serde_json::json;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 || args[1] != "--execute" {
        return Err("usage: firmware_connected --execute EXACT_UID EXACT_STLINK_SELECTOR".into());
    }
    let uid = Uid::try_from(args[2].clone())?;
    let profile = tempfile::tempdir()?;
    let app = AppService::new(profile.path().into())?;
    let discovery = app.start_discovery();
    let result=async{
        tokio::time::timeout(std::time::Duration::from_secs(20),async{loop{if app.inventory.read().channels.iter().any(|c|c.uid==uid&&c.connected){break;}tokio::time::sleep(std::time::Duration::from_millis(200)).await;}}).await?;
        app.select(Some(uid.clone())).await?;let owner=Principal{key_id:"connected-review".into(),client_id:"local".into(),cancelled:Default::default(),ended:Default::default()};
        let inspection=app.execute_firmware(None,&owner,"firmware_inspect",json!({"target":Target::Stm32,"programmer_id":args[3]})).await?;println!("{}",json!({"inspection":inspection}));
        let job=app.execute_firmware(None,&owner,"firmware_start",json!({"target":"stm32","programmer_id":args[3],"image_id":"minikmb-fs-hid-r61","confirmed":true})).await?;let id=job["job_id"].as_str().ok_or("missing job id")?;
        // Accepted writes finish verification; a timer never kills an erase.
        let completed=loop{let info=app.firmware.job(&owner,id)?;if info.terminal(){break info;}tokio::time::sleep(std::time::Duration::from_millis(250)).await;};
        println!("{}",json!({"job":completed}));if completed.result.is_none(){return Err::<_,Box<dyn std::error::Error>>("firmware programming failed; see job result".into());}
        app.request_rescan();
        let _=tokio::time::timeout(std::time::Duration::from_secs(20),async{
            loop{if app.inventory.read().channels.iter().any(|c|c.uid==uid&&c.connected){break;}
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;}
        }).await;
        let channel=app.inventory.read().channels.iter().find(|c|c.uid==uid&&c.connected).cloned();println!("{}",json!({"management_after":channel}));
        if channel.is_none(){return Err("MCU management USB did not return in the observation window".into());}Ok(())
    }.await;
    app.shutdown().await;
    let _ = discovery.join();
    result
}
