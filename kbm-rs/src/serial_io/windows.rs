use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::Arc,
    time::{Duration, Instant},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Devices::Communication::{
            ClearCommError, GetCommState, SetCommState, SetCommTimeouts, CLEAR_COMM_ERROR_FLAGS,
            COMMTIMEOUTS, COMSTAT, DCB, NOPARITY, ONESTOPBIT,
        },
        Foundation::{
            CloseHandle, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, GENERIC_READ, GENERIC_WRITE,
            HANDLE, WAIT_FAILED,
        },
        Storage::FileSystem::{
            CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, OPEN_EXISTING,
        },
        System::{
            Threading::{CreateEventW, ResetEvent, SetEvent, WaitForMultipleObjects},
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        },
    },
};

fn error(e: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(e.code().0 & 0xffff)
}
struct Handle(HANDLE);
// Kernel handles may cross threads; ownership prevents closing during a wait.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
impl Handle {
    fn event(manual: bool) -> io::Result<Self> {
        unsafe {
            CreateEventW(None, manual, false, PCWSTR::null())
                .map(Self)
                .map_err(error)
        }
    }
}
#[derive(Clone)]
pub(crate) struct Wake(Arc<Handle>);
impl Wake {
    pub fn new() -> io::Result<Self> {
        Ok(Self(Arc::new(Handle::event(false)?)))
    }
    pub fn signal(&self) {
        unsafe {
            let _ = SetEvent(self.0 .0);
        }
    }
}
struct Operation {
    ov: OVERLAPPED,
    event: Handle,
    buffer: [u8; 8192],
    pending: bool,
    len: usize,
    offset: usize,
}
// Each OVERLAPPED and its buffer live in a Box and never move while submitted.
unsafe impl Send for Operation {}
impl Operation {
    fn new() -> io::Result<Box<Self>> {
        let event = Handle::event(true)?;
        Ok(Box::new(Self {
            ov: OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            },
            event,
            buffer: [0; 8192],
            pending: false,
            len: 0,
            offset: 0,
        }))
    }
    fn reset(&mut self) -> io::Result<()> {
        debug_assert!(!self.pending);
        unsafe {
            ResetEvent(self.event.0).map_err(error)?;
        }
        self.ov = OVERLAPPED {
            hEvent: self.event.0,
            ..Default::default()
        };
        Ok(())
    }
    fn complete(&mut self, handle: HANDLE) -> io::Result<Option<usize>> {
        if !self.pending {
            return Ok(None);
        }
        let mut n = 0;
        match unsafe { GetOverlappedResult(handle, &self.ov, &mut n, false) } {
            Ok(()) => {
                self.pending = false;
                Ok(Some(n as usize))
            }
            Err(e) if e.code() == ERROR_IO_INCOMPLETE.to_hresult() => Ok(None),
            Err(e) => {
                self.pending = false;
                Err(error(e))
            }
        }
    }
}
pub(crate) struct Port {
    handle: Handle,
    rx: Box<Operation>,
    tx: Box<Operation>,
    serial: bool,
    synchronous_wait: Duration,
    read_ready: VecDeque<u8>,
}
impl Port {
    pub fn open(path: &str) -> io::Result<Self> {
        let path: Vec<u16> = format!("\\\\.\\{path}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let handle = Handle(unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
            .map_err(error)?
        });
        unsafe {
            let mut dcb = DCB {
                DCBlength: std::mem::size_of::<DCB>() as u32,
                ..Default::default()
            };
            GetCommState(handle.0, &mut dcb).map_err(error)?;
            dcb.BaudRate = 115200;
            dcb.ByteSize = 8;
            dcb.Parity = NOPARITY;
            dcb.StopBits = ONESTOPBIT;
            // Binary, no RTS/CTS, XON/XOFF, parity or DSR gating; preserve DTR.
            dcb._bitfield = (dcb._bitfield & (3 << 4)) | 1;
            SetCommState(handle.0, &dcb).map_err(error)?;
            SetCommTimeouts(handle.0, &COMMTIMEOUTS::default()).map_err(error)?;
        }
        Self::from_handle(handle, true)
    }
    fn from_handle(handle: Handle, serial: bool) -> io::Result<Self> {
        Ok(Self {
            handle,
            rx: Operation::new()?,
            tx: Operation::new()?,
            serial,
            synchronous_wait: Duration::from_secs(1),
            read_ready: VecDeque::new(),
        })
    }
    fn begin_read(&mut self) -> io::Result<()> {
        if self.rx.pending {
            return Ok(());
        }
        let count = if self.serial {
            let mut flags = CLEAR_COMM_ERROR_FLAGS::default();
            let mut stat = COMSTAT::default();
            unsafe {
                ClearCommError(self.handle.0, Some(&mut flags), Some(&mut stat)).map_err(error)?;
            }
            if flags.0 != 0 {
                return Err(io::Error::other("COM receive error"));
            }
            (stat.cbInQue as usize).clamp(1, self.rx.buffer.len())
        } else {
            self.rx.buffer.len()
        };
        self.rx.reset()?;
        self.rx.pending = true;
        match unsafe {
            ReadFile(
                self.handle.0,
                Some(&mut self.rx.buffer[..count]),
                None,
                Some(&mut self.rx.ov),
            )
        } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => Ok(()),
            Err(e) => {
                self.rx.pending = false;
                Err(error(e))
            }
        }
    }
    fn begin_write(&mut self) -> io::Result<()> {
        self.tx.reset()?;
        self.tx.pending = true;
        match unsafe {
            WriteFile(
                self.handle.0,
                Some(&self.tx.buffer[self.tx.offset..self.tx.len]),
                None,
                Some(&mut self.tx.ov),
            )
        } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => Ok(()),
            Err(e) => {
                self.tx.pending = false;
                Err(error(e))
            }
        }
    }
    pub fn can_write(&self) -> bool {
        self.tx.len == 0
    }
    pub fn start_write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if !self.can_write() {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        if bytes.is_empty() || bytes.len() > self.tx.buffer.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        self.tx.buffer[..bytes.len()].copy_from_slice(bytes);
        self.tx.len = bytes.len();
        self.tx.offset = 0;
        self.begin_write()
    }
    /// Completed RX bytes and whether the entire TX frame finished.
    pub fn poll(&mut self) -> io::Result<(Vec<u8>, bool)> {
        let mut written = false;
        if let Some(n) = self.tx.complete(self.handle.0)? {
            if n == 0 || n > self.tx.len - self.tx.offset {
                return Err(io::ErrorKind::WriteZero.into());
            }
            self.tx.offset += n;
            if self.tx.offset == self.tx.len {
                self.tx.len = 0;
                written = true;
            } else {
                self.begin_write()?;
            } // Only the remaining suffix, never replay a frame.
        }
        let mut bytes: Vec<u8> = self.read_ready.drain(..).collect();
        for _ in 0..8 {
            self.begin_read()?;
            let Some(n) = self.rx.complete(self.handle.0)? else {
                break;
            };
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            bytes.extend_from_slice(&self.rx.buffer[..n]);
        }
        self.begin_read()?;
        Ok((bytes, written))
    }
    pub fn wait(&self, wake: Option<&Wake>, duration: Duration) -> io::Result<()> {
        let mut handles = Vec::with_capacity(3);
        if let Some(wake) = wake {
            handles.push(wake.0 .0);
        }
        if self.rx.pending {
            handles.push(self.rx.event.0);
        }
        if self.tx.pending {
            handles.push(self.tx.event.0);
        }
        if handles.is_empty() {
            return Ok(());
        }
        let result = unsafe {
            WaitForMultipleObjects(
                &handles,
                false,
                duration.as_millis().min(u32::MAX as u128 - 1) as u32,
            )
        };
        if result == WAIT_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(()) // A maintenance deadline is a normal wake, not an I/O failure.
    }
}
impl Drop for Port {
    fn drop(&mut self) {
        // Cancellation is not completion: drain before freeing kernel-owned buffers.
        for op in [&mut self.rx, &mut self.tx] {
            if op.pending {
                unsafe {
                    let _ = CancelIoEx(self.handle.0, Some(&op.ov));
                    let mut n = 0;
                    let _ = GetOverlappedResult(self.handle.0, &op.ov, &mut n, true);
                }
                op.pending = false;
            }
        }
    }
}
// Only startup HELLO/INFO and --inspect use these synchronous adapters.
impl Write for Port {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.start_write(bytes)?;
        let deadline = Instant::now() + self.synchronous_wait;
        while !self.can_write() {
            if let Some(n) = self.tx.complete(self.handle.0)? {
                if n == 0 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                self.tx.offset += n;
                if self.tx.offset == self.tx.len {
                    self.tx.len = 0;
                    break;
                }
                self.begin_write()?;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            if unsafe {
                WaitForMultipleObjects(
                    &[self.tx.event.0],
                    false,
                    remaining.as_millis().min(u32::MAX as u128 - 1) as u32,
                )
            } == WAIT_FAILED
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Read for Port {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if !self.read_ready.is_empty() {
            let n = out.len().min(self.read_ready.len());
            for dst in &mut out[..n] {
                *dst = self.read_ready.pop_front().unwrap();
            }
            return Ok(n);
        }
        self.begin_read()?;
        loop {
            if let Some(n) = self.rx.complete(self.handle.0)? {
                let copied = n.min(out.len());
                out[..copied].copy_from_slice(&self.rx.buffer[..copied]);
                self.read_ready.extend(&self.rx.buffer[copied..n]);
                return Ok(copied);
            }
            self.wait(None, self.synchronous_wait)?;
            if unsafe { WaitForMultipleObjects(&[self.rx.event.0], false, 0) }.0 == 258 {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
        },
    };
    static NEXT: AtomicU32 = AtomicU32::new(0);
    fn pair() -> (Port, Port) {
        let name: Vec<u16> = format!(
            "\\\\.\\pipe\\minikmb-overlapped-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let server = Handle(unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                64,
                64,
                0,
                None,
            )
        });
        assert_ne!(server.0, INVALID_HANDLE_VALUE);
        let mut connect = Operation::new().unwrap();
        let pending = unsafe { ConnectNamedPipe(server.0, Some(&mut connect.ov)) };
        assert!(pending.is_ok() || pending.unwrap_err().code() == ERROR_IO_PENDING.to_hresult());
        let client = Handle(unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
            .unwrap()
        });
        let mut n = 0;
        unsafe {
            GetOverlappedResult(server.0, &connect.ov, &mut n, true).unwrap();
        }
        (
            Port::from_handle(server, false).unwrap(),
            Port::from_handle(client, false).unwrap(),
        )
    }
    #[test]
    fn delayed_io_preserves_tx_buffer_while_rx_completes_and_wake_interrupts_wait() {
        let (mut a, mut b) = pair();
        let payload: Vec<u8> = (0..=255).cycle().take(1057).collect();
        a.start_write(&payload).unwrap();
        assert!(!a.can_write());
        assert_eq!(
            a.start_write(&[9]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        // Longer than the former 2ms limit, with no read from the peer yet.
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(&a.tx.buffer[..payload.len()], payload);
        b.start_write(&[7, 8, 9]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut rx_a = vec![];
        let mut rx_b = vec![];
        loop {
            rx_a.extend(a.poll().unwrap().0);
            rx_b.extend(b.poll().unwrap().0);
            if rx_a.len() == 3 && rx_b.len() == payload.len() && a.can_write() && b.can_write() {
                break;
            }
            assert!(Instant::now() < deadline);
            a.wait(None, Duration::from_millis(5)).unwrap();
        }
        assert_eq!(rx_a, [7, 8, 9]);
        assert_eq!(rx_b, payload);
        let wake = Wake::new().unwrap();
        let other = wake.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(25));
            other.signal();
        });
        let began = Instant::now();
        a.wait(Some(&wake), Duration::from_secs(2)).unwrap();
        assert!(began.elapsed() < Duration::from_secs(1));
        t.join().unwrap();
        // Dropping both sides with a pending RX must cancel/drain safely.
        drop(a);
        drop(b);
    }
    #[test]
    fn startup_small_reads_keep_remaining_bytes() {
        let (mut a, mut b) = pair();
        b.write_all(&[1, 2, 3, 4, 5]).unwrap();
        let mut byte = [0];
        let mut result = vec![];
        for _ in 0..5 {
            assert_eq!(a.read(&mut byte).unwrap(), 1);
            result.push(byte[0]);
        }
        assert_eq!(result, [1, 2, 3, 4, 5]);
    }
}
