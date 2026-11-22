//! IPv4 LAN enumeration only. No adapter, route or firewall modification.
use super::*;
use ::windows::Win32::{
    Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_NO_DATA},
    NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
    Networking::WinSock::{AF_INET, SOCKADDR_IN},
};
pub(super) fn adapters() -> Result<Vec<Adapter>, AppError> {
    let mut size = 15 * 1024u32;
    for _ in 0..3 {
        if size > 1024 * 1024 {
            return Err(io_error());
        }
        // u64 storage provides the alignment required by IP_ADAPTER_ADDRESSES.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let head = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let result =
            unsafe { GetAdaptersAddresses(AF_INET.0 as u32, flags, None, Some(head), &mut size) };
        if result == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if result == ERROR_NO_DATA.0 {
            return Ok(vec![]);
        }
        if result != 0 {
            return Err(io_error());
        }
        let mut output = vec![];
        let mut current = head;
        while !current.is_null() {
            // All pointers below belong to the successful OS-owned layout in buffer.
            let adapter = unsafe { &*current };
            if adapter.IfType != IF_TYPE_SOFTWARE_LOOPBACK && adapter.OperStatus == IfOperStatusUp {
                let name = if adapter.FriendlyName.is_null() {
                    String::new()
                } else {
                    unsafe { adapter.FriendlyName.to_string() }.map_err(|_| io_error())?
                };
                let mut unicast = adapter.FirstUnicastAddress;
                while !unicast.is_null() {
                    let value = unsafe { &*unicast };
                    let socket = value.Address;
                    if !socket.lpSockaddr.is_null()
                        && socket.iSockaddrLength as usize >= std::mem::size_of::<SOCKADDR_IN>()
                        && unsafe { (*socket.lpSockaddr).sa_family } == AF_INET
                    {
                        let address = unsafe { &*socket.lpSockaddr.cast::<SOCKADDR_IN>() };
                        let ip =
                            Ipv4Addr::from(unsafe { address.sin_addr.S_un.S_addr }.to_ne_bytes());
                        if let Some(broadcast) = directed_broadcast(ip, value.OnLinkPrefixLength) {
                            output.push(Adapter {
                                name: name.clone(),
                                address: ip.to_string(),
                                broadcast: broadcast.to_string(),
                            });
                            if output.len() > 256 {
                                return Err(io_error());
                            }
                        }
                    }
                    unicast = value.Next;
                }
            }
            current = adapter.Next;
        }
        output.sort_by(|a, b| (&a.name, &a.address).cmp(&(&b.name, &b.address)));
        return Ok(output);
    }
    Err(io_error())
}
