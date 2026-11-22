//! Windows management COM I/O: independent overlapped RX/TX and a command wake event.
//! No short I/O timeout is used as a disconnect detector.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{Port, Wake};

#[cfg(not(windows))]
compile_error!("The native management transport currently requires Windows overlapped I/O");
