//! Small Win32 security boundary. SDDL grants access to the current user only.
use std::{ffi::c_void, io, mem, ptr};

type Handle = *mut c_void;
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(
        token: Handle,
        class: u32,
        buffer: *mut c_void,
        size: u32,
        needed: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *mut c_void, string: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn LocalFree(memory: Handle) -> Handle;
    fn GetNamedPipeServerProcessId(pipe: Handle, pid: *mut u32) -> i32;
}
struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct LocalMemory(Handle);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
#[repr(C)]
struct TokenUser {
    sid: *mut c_void,
    attributes: u32,
}

fn sid_of(process: Handle) -> io::Result<String> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(process, 0x0008, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(token);
        let mut needed = 0;
        GetTokenInformation(token.0, 1, ptr::null_mut(), 0, &mut needed);
        if needed < mem::size_of::<TokenUser>() as u32 {
            return Err(io::Error::last_os_error());
        }
        // usize storage gives TOKEN_USER proper alignment.
        let mut buffer = vec![0usize; (needed as usize).div_ceil(mem::size_of::<usize>())];
        if GetTokenInformation(token.0, 1, buffer.as_mut_ptr().cast(), needed, &mut needed) == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TokenUser>();
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(user.sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let _memory = LocalMemory(text.cast());
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(text, length))
            .map_err(|_| io::Error::other("Invalid account SID"))
    }
}
pub(super) fn current_sid() -> io::Result<String> {
    unsafe { sid_of(GetCurrentProcess()) }
}

pub(super) fn verify_server(pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = OpenProcess(0x1000, 0, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = OwnedHandle(process);
        if sid_of(process.0)? != current_sid()? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "IPC server belongs to another Windows user",
            ));
        }
    }
    Ok(())
}

pub(super) fn create_pipe(
    name: &str,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;
    super::validate_endpoint(name)?;
    let text: Vec<u16> = format!("D:P(A;;GA;;;{})\0", current_sid()?)
        .encode_utf16()
        .collect();
    #[repr(C)]
    struct Attributes {
        length: u32,
        descriptor: *mut c_void,
        inherit: i32,
    }
    unsafe {
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _memory = LocalMemory(descriptor);
        let mut attrs = Attributes {
            length: mem::size_of::<Attributes>() as u32,
            descriptor,
            inherit: 0,
        };
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(33)
            .in_buffer_size(64 * 1024)
            .out_buffer_size(64 * 1024)
            .create_with_security_attributes_raw(name, (&mut attrs as *mut Attributes).cast())
    }
}
