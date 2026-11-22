//! Host sockets use the existing routing table. No NIC, firewall, route or ICS writes.
use crate::model::AppError;
use std::{io, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

pub(super) const CHUNKS: usize = 8;
#[derive(Clone, Default)]
pub(super) struct Retired(Arc<parking_lot::Mutex<Vec<RetiredIo>>>);
type RetiredIo = (
    tokio::task::JoinHandle<io::Result<()>>,
    super::flow_budget::Witness,
);
impl Retired {
    fn push(
        &self,
        task: tokio::task::JoinHandle<io::Result<()>>,
        witness: super::flow_budget::Witness,
    ) {
        let mut tasks = self.0.lock();
        tasks.retain(|(task, witness)| !task.is_finished() && witness.live());
        if !task.is_finished() && witness.live() {
            debug_assert!(tasks.len() < super::flow_budget::MAX);
            tasks.push((task, witness));
        }
    }
    pub async fn finish(&self) {
        let tasks = std::mem::take(&mut *self.0.lock());
        for (task, _) in tasks {
            let _ = task.await;
        }
    }
}
pub(super) enum Message {
    Data(Vec<u8>),
    Fin,
}
pub(super) struct Pipe {
    pub tx: mpsc::Sender<Message>,
    pub rx: mpsc::Receiver<Message>,
    task: Option<tokio::task::JoinHandle<io::Result<()>>>,
    life: CancellationToken,
    retired: Option<Retired>,
    witness: Option<super::flow_budget::Witness>,
    result: Option<bool>,
    pub dropped: Arc<std::sync::atomic::AtomicU64>,
}
impl Pipe {
    pub fn retire_to(mut self, retired: Retired) -> Self {
        self.retired = Some(retired);
        self
    }
    pub fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(|task| task.is_finished())
    }
    pub fn shutdown(mut self) -> Option<tokio::task::JoinHandle<io::Result<()>>> {
        self.life.cancel();
        let task = self.task.take();
        if let Some(task) = &task {
            task.abort();
        }
        task
    }
    pub fn succeeded(&mut self) -> bool {
        !self.failed() && self.result == Some(true)
    }
    pub fn failed(&mut self) -> bool {
        if self.result.is_none() && self.is_finished() {
            let mut task = self.task.take().expect("owned pipe task");
            self.result = Some(matches!(
                futures_util::FutureExt::now_or_never(&mut task),
                Some(Ok(Ok(())))
            ));
        }
        self.result == Some(false)
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        self.life.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
            if let Some(retired) = &self.retired {
                retired.push(task, self.witness.take().expect("budgeted retirement"));
            }
        }
    }
}
/// Tests replace destinations with owned loopback endpoints; production is identity.
pub trait Dial: Send + Sync {
    fn address(&self, address: SocketAddr) -> SocketAddr;
}
pub struct NativeDial;
impl Dial for NativeDial {
    fn address(&self, a: SocketAddr) -> SocketAddr {
        a
    }
}
#[derive(Clone)]
pub struct Host {
    pub dns: SocketAddr,
    pub dial: Arc<dyn Dial>,
}
impl Host {
    pub async fn native() -> Result<Self, AppError> {
        let dns = tokio::task::spawn_blocking(system_dns)
            .await
            .map_err(|_| AppError::new("DNS_UNAVAILABLE", "관리 PC DNS를 읽지 못했습니다."))??;
        Ok(Self {
            dns,
            dial: Arc::new(NativeDial),
        })
    }
    #[cfg(test)]
    pub(super) fn pipe(&self, protocol: u8, address: SocketAddr, life: CancellationToken) -> Pipe {
        self.pipe_budgeted(protocol, address, life, None)
    }
    pub(super) fn pipe_budgeted(
        &self,
        protocol: u8,
        address: SocketAddr,
        life: CancellationToken,
        permit: Option<super::flow_budget::Permit>,
    ) -> Pipe {
        let witness = permit.as_ref().map(super::flow_budget::Permit::witness);
        let address = self.dial.address(address);
        let (tx, mut from) = mpsc::channel::<Message>(CHUNKS);
        let (to, rx) = mpsc::channel::<Message>(CHUNKS);
        let dropped = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let dropped_io = dropped.clone();
        let task_life = life.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let work = async {
                if protocol == 6 {
                    let socket =
                        tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address))
                            .await??;
                    socket.set_nodelay(true)?;
                    let (mut r, mut w) = socket.into_split();
                    let down = async {
                        loop {
                            let permit =
                                to.reserve().await.map_err(|_| io::ErrorKind::BrokenPipe)?;
                            let mut data = vec![0; 4096];
                            let n = r.read(&mut data).await?;
                            if n == 0 {
                                permit.send(Message::Fin);
                                return Ok::<_, io::Error>(());
                            }
                            data.truncate(n);
                            permit.send(Message::Data(data));
                        }
                    };
                    let up = async {
                        while let Some(message) = from.recv().await {
                            match message {
                                Message::Data(b) => w.write_all(&b).await?,
                                Message::Fin => {
                                    w.shutdown().await?;
                                    return Ok::<_, io::Error>(());
                                }
                            }
                        }
                        Err(io::ErrorKind::BrokenPipe.into())
                    };
                    tokio::try_join!(up, down)?;
                } else {
                    let bind = if address.is_ipv4() {
                        "0.0.0.0:0"
                    } else {
                        "[::]:0"
                    };
                    let socket = UdpSocket::bind(bind).await?;
                    tokio::time::timeout(Duration::from_secs(2), socket.connect(address)).await??;
                    let down = async {
                        loop {
                            let mut data = vec![0; 1473];
                            let n = socket.recv(&mut data).await?;
                            if n > 1472 {
                                dropped_io.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                continue;
                            }
                            data.truncate(n);
                            // UDP has a finite datagram queue; full queues drop instead of allocating.
                            match to.try_send(Message::Data(data)) {
                                Ok(()) => {}
                                Err(mpsc::error::TrySendError::Full(_)) => {
                                    dropped_io.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                }
                                Err(_) => {
                                    return Err::<(), io::Error>(io::ErrorKind::BrokenPipe.into())
                                }
                            }
                        }
                    };
                    let up = async {
                        while let Some(message) = from.recv().await {
                            if let Message::Data(b) = message {
                                socket.send(&b).await?;
                            }
                        }
                        Err::<(), io::Error>(io::ErrorKind::BrokenPipe.into())
                    };
                    tokio::try_join!(up, down)?;
                }
                Ok(())
            };
            tokio::select! { biased; _=task_life.cancelled()=>Ok(()), r=work=>r }
        });
        Pipe {
            tx,
            rx,
            task: Some(task),
            life,
            retired: None,
            witness,
            result: None,
            dropped,
        }
    }
}

impl Pipe {
    pub(super) fn accepted(
        socket: TcpStream,
        life: CancellationToken,
        permit: super::flow_budget::Permit,
    ) -> Self {
        let witness = Some(permit.witness());
        let (tx, mut from) = mpsc::channel::<Message>(CHUNKS);
        let (to, rx) = mpsc::channel(CHUNKS);
        let task_life = life.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let work = async {
                socket.set_nodelay(true)?;
                let (mut r, mut w) = socket.into_split();
                let down = async {
                    loop {
                        let permit = to.reserve().await.map_err(|_| io::ErrorKind::BrokenPipe)?;
                        let mut b = vec![0; 4096];
                        let n = r.read(&mut b).await?;
                        if n == 0 {
                            permit.send(Message::Fin);
                            return Ok::<(), io::Error>(());
                        }
                        b.truncate(n);
                        permit.send(Message::Data(b));
                    }
                };
                let up = async {
                    while let Some(message) = from.recv().await {
                        match message {
                            Message::Data(b) => w.write_all(&b).await?,
                            Message::Fin => {
                                w.shutdown().await?;
                                return Ok::<(), io::Error>(());
                            }
                        }
                    }
                    Err(io::ErrorKind::BrokenPipe.into())
                };
                tokio::try_join!(up, down)?;
                Ok(())
            };
            tokio::select! {biased;_=task_life.cancelled()=>Ok(()),result=work=>result}
        });
        Self {
            tx,
            rx,
            task: Some(task),
            life,
            retired: None,
            witness,
            result: None,
            dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }
    pub(super) fn udp_client(
        socket: Arc<UdpSocket>,
        remote: SocketAddr,
        life: CancellationToken,
        permit: super::flow_budget::Permit,
    ) -> (Self, mpsc::Sender<Message>) {
        let witness = Some(permit.witness());
        let (tx, mut from) = mpsc::channel::<Message>(CHUNKS);
        let (incoming, rx) = mpsc::channel(CHUNKS);
        let task_life = life.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let work = async {
                while let Some(message) = from.recv().await {
                    if let Message::Data(b) = message {
                        socket.send_to(&b, remote).await?;
                    }
                }
                Ok(())
            };
            tokio::select! {biased;_=task_life.cancelled()=>Ok(()),result=work=>result}
        });
        (
            Self {
                tx,
                rx,
                task: Some(task),
                life,
                retired: None,
                witness,
                result: None,
                dropped: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            },
            incoming,
        )
    }
}
#[cfg(windows)]
fn system_dns() -> Result<SocketAddr, AppError> {
    super::host_dns::system_dns()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn bounded_pipe_queues_and_drop_close_the_owned_idle_host_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = Host {
            dns: addr,
            dial: Arc::new(NativeDial),
        };
        let pipe = host.pipe(6, addr, CancellationToken::new());
        for _ in 0..CHUNKS {
            pipe.tx
                .try_send(Message::Data(vec![7; 4096]))
                .unwrap_or_else(|_| panic!("queue filled early"));
        }
        assert!(matches!(
            pipe.tx.try_send(Message::Fin),
            Err(mpsc::error::TrySendError::Full(_))
        ));
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut bytes = vec![0; CHUNKS * 4096];
        tokio::time::timeout(Duration::from_secs(3), stream.read_exact(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert!(bytes.iter().all(|b| *b == 7));
        drop(pipe);
        let mut one = [0; 1];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), stream.read(&mut one))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
    #[tokio::test]
    async fn parent_cancellation_closes_host_socket_while_the_stack_still_holds_its_pipe() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let life = CancellationToken::new();
        let host = Host {
            dns: addr,
            dial: Arc::new(NativeDial),
        };
        let mut pipe = host.pipe(6, addr, life.child_token());
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        life.cancel();
        let mut b = [0; 1];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), stream.read(&mut b))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert!(!pipe.failed());
    }
}
#[cfg(not(windows))]
fn system_dns() -> Result<SocketAddr, AppError> {
    Err(AppError::new(
        "UNSUPPORTED_PLATFORM",
        "호스트 DNS 조회는 Windows에서 지원합니다.",
    ))
}
