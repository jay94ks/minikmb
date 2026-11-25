use crate::{input_sessions::Principal, service::AppService};
use std::{io, path::Path, sync::Arc, time::Duration};
use tokio::{
    sync::Semaphore,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

pub struct Server {
    pub endpoint: String,
    app: Arc<AppService>,
    principal: Principal,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
impl Server {
    pub async fn start(app: Arc<AppService>, profile: &Path) -> io::Result<Self> {
        let name = super::endpoint(profile)?;
        let listener = super::windows::create_pipe(&name, true)?;
        let stop = CancellationToken::new();
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let principal = Principal {
            key_id: format!("ipc:{:032x}", u128::from_le_bytes(random)),
            client_id: "ipc".into(),
            cancelled: stop.child_token(),
            ended: stop.child_token(),
        };
        let router = crate::remote::ipc_router(app.clone(), principal.clone());
        let token = stop.clone();
        let path = name.clone();
        let task = tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(32));
            let mut jobs = JoinSet::new();
            let mut listener = listener;
            loop {
                let permit = tokio::select! {
                    biased;
                    _=token.cancelled()=>break,
                    permit=slots.clone().acquire_owned()=>match permit { Ok(p)=>p, Err(_)=>break },
                };
                let connected = tokio::select! {
                    biased;
                    _=token.cancelled()=>break,
                    result=listener.connect()=>result,
                };
                if connected.is_err() {
                    break;
                }
                // Keep one listening instance alive before handing off the accepted connection.
                let next = match super::windows::create_pipe(&path, false) {
                    Ok(p) => p,
                    Err(_) => break,
                };
                let pipe = std::mem::replace(&mut listener, next);
                let service = hyper_util::service::TowerToHyperService::new(router.clone());
                let ended = token.clone();
                jobs.spawn(async move {
                    let _permit = permit;
                    let builder = hyper::server::conn::http1::Builder::new();
                    let connection =
                        builder.serve_connection(hyper_util::rt::TokioIo::new(pipe), service);
                    tokio::pin!(connection);
                    tokio::select! {
                        _=ended.cancelled()=>{
                            connection.as_mut().graceful_shutdown();
                            let _=tokio::time::timeout(Duration::from_secs(1), connection).await;
                        },
                        _=&mut connection=>{}
                    }
                });
                while jobs.try_join_next().is_some() {}
            }
            token.cancel();
            while jobs.join_next().await.is_some() {}
        });
        Ok(Self {
            endpoint: name,
            app,
            principal,
            stop,
            task,
        })
    }
    pub async fn shutdown(mut self) {
        self.stop.cancel();
        self.app
            .close_client_inputs(&self.principal.key_id, &self.principal.client_id)
            .await;
        if tokio::time::timeout(Duration::from_secs(2), &mut self.task)
            .await
            .is_err()
        {
            self.task.abort();
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
