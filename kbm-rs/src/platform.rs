use crate::topology::UsbNode;
use std::io;

#[cfg(windows)]
mod win {
    use super::*;
    #[repr(C)]
    struct Guid {
        a: u32,
        b: u16,
        c: u16,
        d: [u8; 8],
    }
    #[repr(C)]
    struct Key {
        guid: Guid,
        pid: u32,
    }
    const DEVICE: Guid = Guid {
        a: 0xa45c254e,
        b: 0xdf1c,
        c: 0x4efd,
        d: [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0],
    };
    #[link(name = "cfgmgr32")]
    unsafe extern "system" {
        fn CM_Get_Device_ID_List_SizeW(size: *mut u32, filter: *const u16, flags: u32) -> u32;
        fn CM_Get_Device_ID_ListW(
            filter: *const u16,
            buffer: *mut u16,
            len: u32,
            flags: u32,
        ) -> u32;
        fn CM_Locate_DevNodeW(node: *mut u32, id: *const u16, flags: u32) -> u32;
        fn CM_Get_Parent(parent: *mut u32, node: u32, flags: u32) -> u32;
        fn CM_Get_Device_IDW(node: u32, id: *mut u16, len: u32, flags: u32) -> u32;
        fn CM_Get_DevNode_PropertyW(
            node: u32,
            key: *const Key,
            ty: *mut u32,
            data: *mut u8,
            len: *mut u32,
            flags: u32,
        ) -> u32;
    }
    unsafe fn property(node: u32, key: Key) -> Vec<u8> {
        let mut ty = 0;
        let mut size = 0;
        unsafe {
            CM_Get_DevNode_PropertyW(node, &key, &mut ty, std::ptr::null_mut(), &mut size, 0);
        }
        if size == 0 || size > 65536 {
            return Vec::new();
        }
        let mut data = vec![0; size as usize];
        if unsafe { CM_Get_DevNode_PropertyW(node, &key, &mut ty, data.as_mut_ptr(), &mut size, 0) }
            == 0
        {
            data.truncate(size as usize);
            data
        } else {
            Vec::new()
        }
    }
    fn strings(data: &[u8]) -> Vec<String> {
        let wide: Vec<_> = data
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        wide.split(|&c| c == 0)
            .filter(|s| !s.is_empty())
            .map(String::from_utf16_lossy)
            .collect()
    }
    pub fn nodes() -> io::Result<Vec<UsbNode>> {
        let filter = [85u16, 83, 66, 0];
        let mut size = 0;
        // All buffers have checked upper bounds and are valid for each Win32 call.
        unsafe {
            if CM_Get_Device_ID_List_SizeW(&mut size, filter.as_ptr(), 0x101) != 0
                || size > 1_048_576
            {
                return Err(io::Error::other("USB enumeration failed"));
            }
            let mut ids = vec![0u16; size as usize];
            if CM_Get_Device_ID_ListW(filter.as_ptr(), ids.as_mut_ptr(), size, 0x101) != 0 {
                return Err(io::Error::other("USB enumeration changed"));
            }
            let mut result = Vec::new();
            for id in ids.split(|&c| c == 0).filter(|s| !s.is_empty()) {
                let text = String::from_utf16_lossy(id);
                let lower = text.to_ascii_lowercase();
                if lower.contains("&mi_")
                    || ![
                        "usb\\vid_cafe&pid_4001",
                        "usb\\vid_cafe&pid_4002",
                        "usb\\vid_cafe&pid_4004",
                        "usb\\vid_345f&pid_2130",
                        "usb\\vid_534d&pid_2130",
                        "usb\\vid_05e3&pid_0610",
                        "usb\\vid_05e3&pid_0626",
                    ]
                    .iter()
                    .any(|p| lower.starts_with(p))
                {
                    continue;
                }
                let mut idz = id.to_vec();
                idz.push(0);
                let mut node = 0;
                if CM_Locate_DevNodeW(&mut node, idz.as_ptr(), 0) != 0 {
                    continue;
                }
                let mut parent = 0;
                let mut parent_id = [0u16; 1024];
                if CM_Get_Parent(&mut parent, node, 0) == 0 {
                    CM_Get_Device_IDW(parent, parent_id.as_mut_ptr(), 1024, 0);
                }
                let key = |pid| Key {
                    guid: Guid { ..DEVICE },
                    pid,
                };
                let address = property(node, key(30));
                let container = property(
                    node,
                    Key {
                        guid: Guid {
                            a: 0x8c7ed206,
                            b: 0x3f8a,
                            c: 0x4827,
                            d: [0xb3, 0xab, 0xae, 0x9e, 0x1f, 0xae, 0xfc, 0x6c],
                        },
                        pid: 2,
                    },
                );
                let container = if container.len() == 16 {
                    container
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<String>()
                } else {
                    String::new()
                };
                result.push(UsbNode {
                    id: text,
                    parent: String::from_utf16_lossy(
                        parent_id.split(|&c| c == 0).next().unwrap_or_default(),
                    ),
                    container,
                    location: strings(&property(node, key(37))),
                    address: if address.len() == 4 {
                        u32::from_le_bytes(address.try_into().unwrap())
                    } else {
                        0
                    },
                    name: strings(&property(node, key(14))).join(""),
                });
            }
            Ok(result)
        }
    }
    pub fn parent(instance: &str) -> io::Result<String> {
        let id: Vec<_> = instance.encode_utf16().chain(Some(0)).collect();
        let mut node = 0;
        let mut parent = 0;
        let mut name = [0u16; 1024];
        // The owned UTF-16 buffers remain valid until all three calls return.
        unsafe {
            if CM_Locate_DevNodeW(&mut node, id.as_ptr(), 0) != 0
                || CM_Get_Parent(&mut parent, node, 0) != 0
                || CM_Get_Device_IDW(parent, name.as_mut_ptr(), 1024, 0) != 0
            {
                return Err(io::Error::other("USB parent unavailable"));
            }
        }
        Ok(String::from_utf16_lossy(
            name.split(|&c| c == 0).next().unwrap_or_default(),
        ))
    }
}
pub fn usb_nodes() -> io::Result<Vec<UsbNode>> {
    #[cfg(windows)]
    {
        win::nodes()
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}
#[cfg(windows)]
pub fn device_parent(instance: &str) -> io::Result<String> {
    win::parent(instance)
}
