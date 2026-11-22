use super::*;
use crate::network::stack::Engine;
use smoltcp::{
    socket::tcp,
    wire::{IpCidr, Ipv4Cidr},
};
fn owner() -> Principal {
    Principal {
        key_id: "test".into(),
        client_id: "test".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    }
}
fn request(start: u16, end: u16) -> Request {
    Request {
        target: "192.168.127.100".into(),
        start,
        end,
        timeout_ms: 250,
    }
}
const MAC: [u8; 6] = [2, 0x4b, 0x42, 0x4d, 0, 1];
const TARGET_MAC: [u8; 6] = [2, 17, 34, 51, 68, 85];
fn arp_reply() -> Vec<u8> {
    let mut b = vec![0; 42];
    b[..6].copy_from_slice(&MAC);
    b[6..12].copy_from_slice(&TARGET_MAC);
    b[12..22].copy_from_slice(&[8, 6, 0, 1, 8, 0, 6, 4, 0, 2]);
    b[22..28].copy_from_slice(&TARGET_MAC);
    b[28..32].copy_from_slice(&[192, 168, 127, 100]);
    b[32..38].copy_from_slice(&MAC);
    b[38..42].copy_from_slice(&[192, 168, 127, 1]);
    b
}
fn reply(probe: &[u8], flags: u8, delta: u32) -> Vec<u8> {
    let p = ipv4::packet(probe).unwrap();
    let seq = u32::from_be_bytes(probe[38..42].try_into().unwrap());
    ipv4::tcp_frame(
        TARGET_MAC,
        MAC,
        Key {
            protocol: 6,
            client: p.key.remote,
            client_port: p.key.remote_port,
            remote: p.key.client,
            remote_port: p.key.client_port,
        },
        1234,
        seq.wrapping_add(delta),
        flags,
    )
}
#[test]
fn validates_canonical_single_target_range_and_timeout_without_mutating_existing_scan() {
    let mut scanner = Scanner::new(Config::default(), MAC).unwrap();
    scanner
        .start(request(1, 65535), owner(), CancellationToken::new())
        .unwrap();
    for bad in [
        Request {
            target: "127.0.0.1".into(),
            ..request(1, 2)
        },
        Request {
            target: "192.168.127.1".into(),
            ..request(1, 2)
        },
        Request {
            target: "192.168.127.255".into(),
            ..request(1, 2)
        },
        Request {
            target: "192.168.127.0100".into(),
            ..request(1, 2)
        },
        request(0, 2),
        request(3, 2),
        Request {
            timeout_ms: 249,
            ..request(1, 2)
        },
        Request {
            timeout_ms: 5001,
            ..request(1, 2)
        },
    ] {
        assert!(scanner
            .start(bad, owner(), CancellationToken::new())
            .is_err());
        assert!(scanner.status.running);
        assert_eq!(scanner.status.total, 65535);
    }
    assert!(serde_json::from_value::<Request>(
        serde_json::json!({"target":"192.168.127.100","start":1,"end":2,"timeout_ms":250,"uids":[]})
    )
    .is_err());
}
#[test]
fn arp_and_probe_bounds_validate_ack_checksum_mac_and_emit_reset() {
    let mut scanner = Scanner::new(Config::default(), MAC).unwrap();
    scanner
        .start(request(1, 20), owner(), CancellationToken::new())
        .unwrap();
    assert!(scanner.poll(0, 0).is_empty());
    assert_eq!(scanner.poll(0, 1).len(), 1);
    let mut wrong = arp_reply();
    wrong[32] ^= 1;
    assert!(!scanner.receive(&wrong));
    assert!(scanner.poll(1, 32).is_empty());
    assert!(!scanner.receive(&arp_reply()));
    let probes = scanner.poll(2, 32);
    assert_eq!(probes.len(), 8);
    assert_eq!(scanner.status.pending, 8);
    assert!(scanner.poll(3, 32).is_empty());
    assert!(!scanner.receive(&reply(&probes[0].frame, 0x12, 2)));
    let mut bad = reply(&probes[0].frame, 0x12, 1);
    bad[50] ^= 1;
    assert!(!scanner.receive(&bad));
    let mut bad = reply(&probes[0].frame, 0x12, 1);
    bad[6] ^= 2;
    assert!(!scanner.receive(&bad));
    assert!(scanner.receive(&reply(&probes[0].frame, 0x12, 1)));
    assert_eq!(scanner.status.open, [1]);
    let reset = scanner.poll(4, 1);
    assert_eq!(reset.len(), 1);
    let p = ipv4::packet(&reset[0].frame).unwrap();
    assert!(p.valid());
    assert_eq!(p.frame[p.offset + 13], 0x14);
    assert_eq!(p.key.remote_port, 1);
    assert!(scanner.receive(&reply(&probes[1].frame, 0x14, 1)));
    assert_eq!(scanner.status.closed, 1);
    scanner.poll(300, 0);
    assert_eq!(scanner.status.filtered, 6);
    assert_eq!(scanner.status.completed, 8);
    let stopped_scope = probes[0].scope.clone().unwrap();
    scanner.stop();
    assert!(stopped_scope.cancelled());
    assert!(!scanner.receive(&reply(&probes[2].frame, 0x12, 1)));
    assert!(scanner.poll(500, 32).is_empty());
    assert_eq!(scanner.status.completed, 8);
    assert_eq!(scanner.status.phase, "stopped");
}
#[test]
fn arp_timeout_and_owner_revocation_never_start_tcp_probes() {
    let mut scanner = Scanner::new(Config::default(), MAC).unwrap();
    let p = owner();
    scanner
        .start(request(1, 10), p.clone(), CancellationToken::new())
        .unwrap();
    scanner.poll(0, 32);
    scanner.poll(2000, 32);
    assert_eq!(scanner.status.phase, "failed");
    assert_eq!(scanner.status.error.as_ref().unwrap().code, "SCAN_NO_ARP");
    assert_eq!(scanner.status.completed, 0);
    scanner
        .start(request(1, 10), p.clone(), CancellationToken::new())
        .unwrap();
    let arp = scanner.poll(0, 32);
    p.cancelled.cancel();
    assert!(arp[0].scope.as_ref().unwrap().cancelled());
    assert!(scanner.poll(1, 32).is_empty());
    assert_eq!(scanner.status.phase, "cancelled");
}
#[test]
fn actual_smoltcp_target_distinguishes_open_closed_and_filtered_without_host_network() {
    let mut server = Engine::new(Config::default(), MAC, false).unwrap();
    let mut peer = Engine::new(Config::default(), TARGET_MAC, false).unwrap();
    peer.interface.update_ip_addrs(|ips| {
        ips.clear();
        ips.push(IpCidr::Ipv4(Ipv4Cidr::new(
            Ipv4Addr::new(192, 168, 127, 100),
            24,
        )))
        .unwrap();
    });
    let mut listener = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 4096]),
        tcp::SocketBuffer::new(vec![0; 4096]),
    );
    listener.listen(22).unwrap();
    peer.sockets.add(listener);
    server
        .start_scan(request(22, 24), owner(), CancellationToken::new())
        .unwrap();
    let mut syn_count = 0;
    let mut reset_count = 0;
    for now in (0..1000).step_by(5) {
        server.poll(now);
        while let Some(b) = server.output() {
            if let Some(p) = ipv4::packet(&b) {
                if p.syn() {
                    syn_count += 1;
                    if p.key.remote_port == 24 {
                        continue;
                    }
                }
                if p.frame[p.offset + 13] & 4 != 0 {
                    reset_count += 1;
                }
            }
            peer.receive(b, now);
        }
        peer.poll(now);
        while let Some(b) = peer.output() {
            server.receive(b, now);
        }
        if server.scan_status().phase == "completed" {
            break;
        }
    }
    let s = server.scan_status();
    assert_eq!(s.phase, "completed");
    assert_eq!(s.open, [22]);
    assert_eq!(s.closed, 1);
    assert_eq!(s.filtered, 1);
    assert_eq!(s.completed, 3);
    assert_eq!(syn_count, 3);
    assert_eq!(reset_count, 1);
}
