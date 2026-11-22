use super::*;
use crate::protocol::Frame;
use serde_json::Value;
use std::time::{Duration, Instant};
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/v1.json")).unwrap()
}
fn bytes(value: &Value) -> Vec<u8> {
    value
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn event(p: Vec<u8>, seq: u32) -> Frame {
    Frame {
        session: 7,
        kind: 4,
        channel: 4,
        opcode: 0x480,
        sequence: seq,
        payload: p,
    }
}
#[test]
fn matches_actual_electron_v1_fragments_credit_and_dhcp_with_host_dns_only() {
    let f = fixture();
    let frame = bytes(&f["frame"]);
    let calls = f["calls"].as_array().unwrap();
    assert_eq!(framing::credit(framing::WINDOW), bytes(&calls[0]["body"]));
    let expected: Vec<_> = calls
        .iter()
        .filter(|c| c["op"] == 0x402)
        .map(|c| bytes(&c["body"]))
        .collect();
    let mut fragments = framing::fragments(1, &frame).unwrap();
    fragments.extend(framing::fragments(2, &frame).unwrap());
    assert_eq!(fragments, expected);
    let mac: [u8; 6] = serde_json::from_value(f["dhcp"]["mac"].clone()).unwrap();
    let request = bytes(&f["dhcp"]["request"]);
    for host in [false, true] {
        let mut server = dhcp::Server::new(Config::default(), mac, host).unwrap();
        let dhcp::Reply::Consumed(Some(reply)) = server.handle(&request, 123456) else {
            panic!("DHCP discover rejected")
        };
        assert_eq!(
            reply,
            bytes(&f["dhcp"][if host { "host" } else { "offer" }])
        );
        assert_eq!(
            server.leases(123456)[0].ip,
            "192.168.127.100".parse::<std::net::Ipv4Addr>().unwrap()
        );
    }
}
#[test]
fn reassembly_rejects_partial_crc_replay_gap_deadline_and_credit_overrun() {
    let data = bytes(&fixture()["frame"]);
    let now = Instant::now();
    let mut r = framing::Receiver::synchronized(7, 0, framing::WINDOW).unwrap();
    let p = framing::fragments(1, &data).unwrap();
    assert!(r.feed(&event(p[0].clone(), 1), now).unwrap().is_none());
    let before = r.received;
    assert!(r.feed(&event(p[0].clone(), 1), now).unwrap().is_none());
    assert_eq!(r.received, before);
    assert_eq!(r.feed(&event(p[1].clone(), 2), now).unwrap().unwrap(), data);
    let p = framing::fragments(2, &data).unwrap();
    r.feed(&event(p[0].clone(), 3), now).unwrap();
    let mut corrupt = p[1].clone();
    corrupt[16] ^= 1;
    assert!(r.feed(&event(corrupt, 4), now).unwrap().is_none());
    let p = framing::fragments(3, &data).unwrap();
    r.feed(&event(p[0].clone(), 5), now).unwrap();
    assert!(r
        .feed(&event(p[1].clone(), 6), now + Duration::from_secs(1))
        .unwrap()
        .is_none());
    let mut other = event(vec![0; 1024], 99);
    other.session = 8;
    let before = r.received;
    assert!(r.feed(&other, now).unwrap().is_none());
    assert_eq!(r.received, before);
    assert!(r.feed(&event(p[0].clone(), 8), now).is_err());
    let mut small = framing::Receiver::synchronized(7, 0, 12).unwrap();
    assert!(small.feed(&event(p[0].clone(), 1), now).is_err());
    let mut reserved = p[0].clone();
    reserved[12] = 1;
    let mut r = framing::Receiver::synchronized(7, 0, framing::WINDOW).unwrap();
    assert!(r.feed(&event(reserved, 1), now).unwrap().is_none());
    assert!(r.dropped > 0);
}
#[test]
fn absolute_credit_and_sticky_ids_are_not_reset_by_new_receive_window() {
    let mut r = framing::Receiver::synchronized(7, 40000, 50000).unwrap();
    assert_eq!(r.renewal().unwrap(), Some(72768));
    assert_eq!(r.renewal().unwrap(), None);
    let ack = [40000u64.to_le_bytes(), 72768u64.to_le_bytes()].concat();
    assert_eq!(framing::credit_reply(&ack).unwrap(), (40000, 72768));
    assert!(framing::credit_reply(&[0; 15]).is_err());
    assert!(framing::Receiver::synchronized(7, 0, 32769).is_err());
    let data = vec![0; 64];
    let p = framing::fragments(u32::MAX, &data).unwrap();
    assert!(r
        .feed(&event(p[0].clone(), 100), Instant::now())
        .unwrap()
        .is_some());
    assert!(r
        .feed(&event(p[0].clone(), 101), Instant::now())
        .unwrap()
        .is_none());
    assert!(framing::fragments(0, &data).is_err());
    assert!(framing::fragments(1, &[0; 13]).is_err());
    assert!(framing::fragments(1, &[0; 1519]).is_err());
}
fn request(mut p: Vec<u8>, kind: u8, options: &[u8]) -> Vec<u8> {
    p[282..].fill(0);
    p[282..285].copy_from_slice(&[53, 1, kind]);
    p[285..285 + options.len()].copy_from_slice(options);
    p[285 + options.len()] = 255;
    p
}
fn response(server: &mut dhcp::Server, p: &[u8], now: u64) -> Vec<u8> {
    match server.handle(p, now) {
        dhcp::Reply::Consumed(Some(reply)) => reply,
        _ => panic!("expected DHCP reply"),
    }
}
#[test]
fn dhcp_ack_nak_release_pool_expiry_and_malformed_requests() {
    let f = fixture();
    let p = bytes(&f["dhcp"]["request"]);
    let cfg = Config {
        pool_start: 100,
        pool_end: 101,
        lease_seconds: 60,
        ..Default::default()
    };
    let mut server = dhcp::Server::new(cfg, [2, 0x4b, 0x42, 0x4d, 0, 1], false).unwrap();
    response(&mut server, &p, 0);
    let ack = response(
        &mut server,
        &request(
            p.clone(),
            3,
            &[50, 4, 192, 168, 127, 100, 54, 4, 192, 168, 127, 1],
        ),
        1,
    );
    assert_eq!(ack[284], 5);
    assert!(!server.leases(2)[0].offered);
    let nak = response(
        &mut server,
        &request(p.clone(), 3, &[50, 4, 192, 168, 127, 101]),
        3,
    );
    assert_eq!(nak[284], 6);
    let mut second = p.clone();
    second[75] = 86;
    response(&mut server, &second, 4);
    let mut third = p.clone();
    third[75] = 87;
    assert!(matches!(
        server.handle(&third, 5),
        dhcp::Reply::Consumed(None)
    ));
    server.handle(&request(p.clone(), 7, &[]), 6);
    response(&mut server, &third, 7);
    assert_eq!(server.leases(8).len(), 2);
    assert!(server.leases(70000).is_empty());
    let mut bad = p.clone();
    bad[24] ^= 1;
    assert!(matches!(
        server.handle(&bad, 0),
        dhcp::Reply::Consumed(None)
    ));
    let mut relay = p.clone();
    relay[66] = 1;
    assert!(matches!(
        server.handle(&relay, 0),
        dhcp::Reply::Consumed(None)
    ));
    let mut short = p.clone();
    short[283] = 255;
    assert!(matches!(
        server.handle(&short, 0),
        dhcp::Reply::Consumed(None)
    ));
    let mut not_dhcp = p;
    not_dhcp[36] = 0;
    not_dhcp[37] = 22;
    assert!(matches!(server.handle(&not_dhcp, 0), dhcp::Reply::Other));
}
#[test]
fn private_gateway_pool_and_target_validation_matches_v1() {
    for ip in [
        "127.0.0.1",
        "8.8.8.1",
        "192.168.127.2",
        "192.168.127.01",
        "172.15.1.1",
    ] {
        let c = Config {
            server_ip: ip.into(),
            ..Default::default()
        };
        assert!(c.validate().is_err());
    }
    for ip in ["10.0.0.1", "172.16.0.1", "172.31.255.1", "192.168.127.1"] {
        assert!(Config {
            server_ip: ip.into(),
            ..Default::default()
        }
        .validate()
        .is_ok());
    }
    let c = Config::default();
    for ip in [
        "192.168.127.0",
        "192.168.127.1",
        "192.168.127.255",
        "192.168.128.100",
    ] {
        assert!(c.target(ip.parse().unwrap()).is_err());
    }
    assert!(c.target("192.168.127.100".parse().unwrap()).is_ok());
}
