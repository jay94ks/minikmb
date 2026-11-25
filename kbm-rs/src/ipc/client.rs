use axum::{
    body::Body,
    http::{Request, Response},
};
use std::{
    io,
    time::{Duration, Instant},
};
use tokio::net::windows::named_pipe::ClientOptions;

pub async fn request(
    name: &str,
    request: Request<Body>,
    connect_timeout: Duration,
) -> io::Result<Response<hyper::body::Incoming>> {
    super::validate_endpoint(name)?;
    let deadline = Instant::now() + connect_timeout;
    let pipe = loop {
        match ClientOptions::new().open(name) {
            Ok(pipe) => break pipe,
            Err(error) if error.raw_os_error() == Some(231) && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(io::Error::new(error.kind(), format!("Cannot connect to MiniKMB IPC. Start the updated GUI with the same profile. {error}"))),
        }
    };
    super::windows::verify_server(&pipe)?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(pipe))
            .await
            .map_err(io::Error::other)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let response = sender
        .send_request(request)
        .await
        .map_err(io::Error::other)?;
    if response
        .headers()
        .get("x-minikmb-ipc")
        .and_then(|v| v.to_str().ok())
        != Some("1")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported MiniKMB IPC protocol",
        ));
    }
    Ok(response)
}
