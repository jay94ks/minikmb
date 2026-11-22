//! Read-only Windows resolver discovery, including scoped IPv6 DNS endpoints.
use crate::model::AppError;
use std::{
    collections::HashSet,
    mem::{align_of, size_of, size_of_val},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6},
};
use windows::Win32::{
    Foundation::ERROR_BUFFER_OVERFLOW,
    NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
    Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKET_ADDRESS,
    },
};
#[cfg(test)]
mod tests;
fn unavailable() -> AppError {
    AppError::new(
        "DNS_UNAVAILABLE",
        "관리 PC의 IPv4/IPv6 DNS 서버를 찾지 못했습니다.",
    )
}
pub(super) fn system_dns() -> Result<SocketAddr, AppError> {
    let mut bytes = 15 * 1024u32;
    for _ in 0..3 {
        if bytes == 0 || bytes > 1024 * 1024 {
            return Err(unavailable());
        }
        let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
        let head = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let flags = GAA_FLAG_SKIP_UNICAST
            | GAA_FLAG_SKIP_ANYCAST
            | GAA_FLAG_SKIP_MULTICAST
            | GAA_FLAG_SKIP_FRIENDLY_NAME;
        let result = unsafe {
            GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(head), &mut bytes)
        };
        if result == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if result != 0 || bytes as usize > size_of_val(buffer.as_slice()) {
            return Err(unavailable());
        }
        return decode(&buffer, head);
    }
    Err(unavailable())
}
/// T must be one of the initialized Win32 POD structures returned in this buffer.
unsafe fn at<T>(buffer: &[u64], pointer: *const T) -> Result<&T, AppError> {
    let start = buffer.as_ptr() as usize;
    let end = start + size_of_val(buffer);
    let address = pointer as usize;
    if pointer.is_null()
        || address < start
        || !address.is_multiple_of(align_of::<T>())
        || address
            .checked_add(size_of::<T>())
            .is_none_or(|end_of_item| end_of_item > end)
    {
        return Err(unavailable());
    }
    // Caller restricts T to the Win32 layout; bounds/alignment/lifetime are checked here.
    Ok(unsafe { &*pointer })
}
fn ipv4(ip: Ipv4Addr) -> Option<SocketAddr> {
    (!ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_broadcast()
        && ip.octets()[0] != 0
        && ip.octets()[0] < 224)
        .then(|| SocketAddr::from((ip, 53)))
}
fn address(
    buffer: &[u64],
    socket: SOCKET_ADDRESS,
    interface: u32,
) -> Result<Option<SocketAddr>, AppError> {
    if socket.iSockaddrLength < 0 {
        return Err(unavailable());
    }
    let length = socket.iSockaddrLength as usize;
    if length < size_of::<SOCKADDR>() {
        return Err(unavailable());
    }
    let family = unsafe { at(buffer, socket.lpSockaddr)? }.sa_family;
    if family == AF_INET {
        if length < size_of::<SOCKADDR_IN>() {
            return Err(unavailable());
        }
        let socket = unsafe { at(buffer, socket.lpSockaddr.cast::<SOCKADDR_IN>())? };
        return Ok(ipv4(Ipv4Addr::from(
            unsafe { socket.sin_addr.S_un.S_addr }.to_ne_bytes(),
        )));
    }
    if family == AF_INET6 {
        if length < size_of::<SOCKADDR_IN6>() {
            return Err(unavailable());
        }
        let socket = unsafe { at(buffer, socket.lpSockaddr.cast::<SOCKADDR_IN6>())? };
        let ip = Ipv6Addr::from(unsafe { socket.sin6_addr.u.Byte });
        if let Some(mapped) = ip.to_ipv4_mapped() {
            return Ok(ipv4(mapped));
        }
        if ip.is_unspecified() || ip.is_multicast() {
            return Ok(None);
        }
        let mut scope = unsafe { socket.Anonymous.sin6_scope_id };
        if ip.is_unicast_link_local() && scope == 0 {
            scope = interface;
        }
        if ip.is_unicast_link_local() && scope == 0 {
            return Ok(None);
        }
        return Ok(Some(SocketAddr::V6(SocketAddrV6::new(ip, 53, 0, scope))));
    }
    Ok(None)
}
fn decode(
    buffer: &[u64],
    mut adapter: *const IP_ADAPTER_ADDRESSES_LH,
) -> Result<SocketAddr, AppError> {
    let mut adapters = HashSet::new();
    let mut server_count = 0usize;
    // Preserve the OS adapter order and each interface's configured DNS order.
    while !adapter.is_null() {
        if adapters.len() >= 256 || !adapters.insert(adapter as usize) {
            return Err(unavailable());
        }
        let current = unsafe { at(buffer, adapter)? };
        if unsafe { current.Anonymous1.Anonymous.Length }
            < (size_of::<IP_ADAPTER_ADDRESSES_LH>() as u32)
        {
            return Err(unavailable());
        }
        if current.OperStatus == IfOperStatusUp {
            let mut servers = HashSet::new();
            let mut dns = current.FirstDnsServerAddress;
            while !dns.is_null() {
                if server_count >= 4096 || !servers.insert(dns as usize) {
                    return Err(unavailable());
                }
                server_count += 1;
                let item = unsafe { at(buffer, dns)? };
                if unsafe { item.Anonymous.Anonymous.Length }
                    < (size_of::<IP_ADAPTER_DNS_SERVER_ADDRESS_XP>() as u32)
                {
                    return Err(unavailable());
                }
                if let Some(endpoint) = address(buffer, item.Address, current.Ipv6IfIndex)? {
                    return Ok(endpoint);
                }
                dns = item.Next;
            }
        }
        adapter = current.Next;
    }
    Err(unavailable())
}
