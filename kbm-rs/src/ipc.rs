//! HTTP/1 over local Windows named pipes, independent of the TCP REST switch.
use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(windows)]
mod server;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use server::Server;
#[cfg(windows)]
pub mod client;
#[cfg(all(test, windows))]
mod tests;

pub fn default_profile() -> io::Result<PathBuf> {
    directories::ProjectDirs::from("local", "MiniKMB", "MiniKMB Native")
        .map(|p| p.data_local_dir().to_owned())
        .ok_or_else(|| io::Error::other("MiniKMB profile directory is unavailable"))
}

#[cfg(windows)]
pub fn endpoint(profile: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    let path = std::fs::canonicalize(profile).unwrap_or(std::path::absolute(profile)?);
    let profile = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let sid = windows::current_sid()?;
    let hash = Sha256::digest(format!("{sid}\0{profile}"));
    Ok(format!(r"\\.\pipe\MiniKMB.Native.{:x}", hash))
}

#[cfg(not(windows))]
pub fn endpoint(_: &Path) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "MiniKMB IPC currently requires Windows",
    ))
}

pub fn validate_endpoint(name: &str) -> io::Result<()> {
    if name.starts_with(r"\\.\pipe\MiniKMB.Native.")
        && name.len() <= 240
        && name[r"\\.\pipe\MiniKMB.Native.".len()..]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        && name.len() > r"\\.\pipe\MiniKMB.Native.".len()
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Only local MiniKMB named pipes are supported",
        ))
    }
}
