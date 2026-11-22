use super::*;
use windows::Win32::NetworkManagement::Ndis::IfOperStatusDown;
#[test]
#[ignore = "Explicit MINIKMB_DNS_INSPECT=1; reads Windows adapter tables only, sends no packets"]
fn windows_read_only_system_dns_discovery() {
    assert_eq!(std::env::var("MINIKMB_DNS_INSPECT").as_deref(), Ok("1"));
    let endpoint = system_dns().unwrap();
    assert_eq!(endpoint.port(), 53);
    println!(
        "{}",
        serde_json::json!({"family":if endpoint.is_ipv4(){"ipv4"}else{"ipv6"},"scope_present":match endpoint{SocketAddr::V6(a)=>a.scope_id()!=0,_=>false},"packets_sent":false,"nic_changes":false})
    );
}
struct Fixture {
    words: Vec<u64>,
    next: usize,
}
impl Fixture {
    fn new() -> Self {
        Self {
            words: vec![0; 4096],
            next: 0,
        }
    }
    fn put<T: Copy>(&mut self, value: T) -> *mut T {
        assert!(align_of::<T>() <= align_of::<u64>());
        let at = self.next;
        self.next += size_of::<T>().div_ceil(8);
        assert!(self.next < self.words.len());
        let pointer = unsafe { self.words.as_mut_ptr().add(at).cast::<T>() };
        unsafe {
            pointer.write(value);
        }
        pointer
    }
    fn socket(&mut self, ip: &str, scope: u32) -> SOCKET_ADDRESS {
        match ip.parse::<std::net::IpAddr>().unwrap() {
            std::net::IpAddr::V4(ip) => {
                let mut s = SOCKADDR_IN {
                    sin_family: AF_INET,
                    ..Default::default()
                };
                s.sin_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
                SOCKET_ADDRESS {
                    lpSockaddr: self.put(s).cast(),
                    iSockaddrLength: size_of::<SOCKADDR_IN>() as i32,
                }
            }
            std::net::IpAddr::V6(ip) => {
                let mut s = SOCKADDR_IN6 {
                    sin6_family: AF_INET6,
                    ..Default::default()
                };
                s.sin6_addr.u.Byte = ip.octets();
                s.Anonymous.sin6_scope_id = scope;
                SOCKET_ADDRESS {
                    lpSockaddr: self.put(s).cast(),
                    iSockaddrLength: size_of::<SOCKADDR_IN6>() as i32,
                }
            }
        }
    }
    fn dns(&mut self, ip: &str, scope: u32) -> *mut IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
        let mut dns = IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
            Address: self.socket(ip, scope),
            ..Default::default()
        };
        dns.Anonymous.Anonymous.Length = size_of::<IP_ADAPTER_DNS_SERVER_ADDRESS_XP>() as u32;
        self.put(dns)
    }
    fn adapter(
        &mut self,
        dns: *mut IP_ADAPTER_DNS_SERVER_ADDRESS_XP,
        up: bool,
        index: u32,
    ) -> *mut IP_ADAPTER_ADDRESSES_LH {
        let mut a = IP_ADAPTER_ADDRESSES_LH {
            FirstDnsServerAddress: dns,
            OperStatus: if up { IfOperStatusUp } else { IfOperStatusDown },
            Ipv6IfIndex: index,
            ..Default::default()
        };
        a.Anonymous1.Anonymous.Length = size_of::<IP_ADAPTER_ADDRESSES_LH>() as u32;
        self.put(a)
    }
}
#[test]
fn ipv6_only_and_mixed_dns_preserve_os_preference_order() {
    let mut f = Fixture::new();
    let first = f.dns("2001:db8::53", 0);
    let second = f.dns("192.0.2.53", 0);
    unsafe {
        (*first).Next = second;
    }
    let a = f.adapter(first, true, 7);
    assert_eq!(
        decode(&f.words, a).unwrap(),
        "[2001:db8::53]:53".parse::<SocketAddr>().unwrap()
    );
    unsafe {
        (*second).Next = first;
        (*first).Next = std::ptr::null_mut();
        (*a).FirstDnsServerAddress = second;
    }
    assert_eq!(
        decode(&f.words, a).unwrap(),
        "192.0.2.53:53".parse::<SocketAddr>().unwrap()
    );
}
#[test]
fn scoped_link_local_dns_keeps_explicit_scope_or_uses_adapter_index() {
    for (given, index, expected) in [(12, 7, 12), (0, 7, 7)] {
        let mut f = Fixture::new();
        let dns = f.dns("fe80::53", given);
        let a = f.adapter(dns, true, index);
        assert_eq!(
            decode(&f.words, a).unwrap(),
            SocketAddr::V6(SocketAddrV6::new(
                "fe80::53".parse().unwrap(),
                53,
                0,
                expected
            ))
        );
    }
    let mut f = Fixture::new();
    let dns = f.dns("fe80::53", 0);
    let a = f.adapter(dns, true, 0);
    assert_eq!(decode(&f.words, a).unwrap_err().code, "DNS_UNAVAILABLE");
    // A shared list may become usable on the next adapter because it supplies scope.
    let next = f.adapter(dns, true, 7);
    unsafe {
        (*a).Next = next;
    }
    assert_eq!(
        decode(&f.words, a).unwrap(),
        SocketAddr::V6(SocketAddrV6::new("fe80::53".parse().unwrap(), 53, 0, 7))
    );
}
#[test]
fn down_interfaces_and_invalid_addresses_are_skipped_and_local_resolvers_are_valid() {
    let mut f = Fixture::new();
    let ignored = f.dns("192.0.2.1", 0);
    let down = f.adapter(ignored, false, 1);
    let any = f.dns("::", 0);
    let multicast = f.dns("ff02::1", 1);
    let broadcast = f.dns("255.255.255.255", 0);
    let local = f.dns("::1", 0);
    let up = f.adapter(any, true, 2);
    unsafe {
        (*down).Next = up;
        (*any).Next = multicast;
        (*multicast).Next = broadcast;
        (*broadcast).Next = local;
    }
    assert_eq!(
        decode(&f.words, down).unwrap(),
        "[::1]:53".parse::<SocketAddr>().unwrap()
    );
    let mapped = f.dns("::ffff:127.0.0.1", 0);
    unsafe {
        (*up).FirstDnsServerAddress = mapped;
    }
    assert_eq!(
        decode(&f.words, up).unwrap(),
        "127.0.0.1:53".parse::<SocketAddr>().unwrap()
    );
}
#[test]
fn malformed_pointers_lengths_and_cycles_fail_without_unbounded_reads() {
    let mut f = Fixture::new();
    let dns = f.dns("::", 0);
    let a = f.adapter(dns, true, 1);
    unsafe {
        (*dns).Next = dns;
    }
    assert!(decode(&f.words, a).is_err());
    unsafe {
        (*dns).Next = std::ptr::null_mut();
        (*a).Next = a;
    }
    assert!(decode(&f.words, a).is_err());
    unsafe {
        (*a).Next = std::ptr::null_mut();
        (*a).FirstDnsServerAddress =
            (f.words.as_ptr() as usize + size_of_val(f.words.as_slice()) + 8) as *mut _;
    }
    assert!(decode(&f.words, a).is_err());
    unsafe {
        (*a).FirstDnsServerAddress = dns;
        (*dns).Address.iSockaddrLength = 1;
    }
    assert!(decode(&f.words, a).is_err());
    unsafe {
        (*dns).Address.iSockaddrLength = -1;
    }
    assert!(decode(&f.words, a).is_err());
    unsafe {
        (*a).Anonymous1.Anonymous.Length = 4;
    }
    assert!(decode(&f.words, a).is_err());
}
