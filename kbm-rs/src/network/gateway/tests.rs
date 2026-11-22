use super::*;
use crate::network::{framing, host_io::Dial, stack::Engine};
use smoltcp::wire::{IpCidr, Ipv4Cidr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UdpSocket},
};
struct Local(SocketAddr);
impl Dial for Local {
    fn address(&self, _a: SocketAddr) -> SocketAddr {
        self.0
    }
}
fn pair(host: Host) -> (Engine, Engine) {
    let server = Engine::with_host(Config::default(), [2, 0x4b, 0x42, 0x4d, 0, 1], host).unwrap();
    let mut peer = Engine::new(Config::default(), [2, 17, 34, 51, 68, 85], false).unwrap();
    peer.interface.update_ip_addrs(|ips| {
        ips.clear();
        ips.push(IpCidr::Ipv4(Ipv4Cidr::new(
            Ipv4Addr::new(192, 168, 127, 100),
            24,
        )))
        .unwrap();
    });
    peer.interface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Addr::new(192, 168, 127, 1))
        .unwrap();
    (server, peer)
}
fn pump(from: &mut Engine, to: &mut Engine, now: u64, id: &mut u32) {
    while let Some(frame) = from.output() {
        *id += 1;
        let mut receiver = framing::Receiver::synchronized(7, 0, framing::WINDOW).unwrap();
        for (i, payload) in framing::fragments(*id, &frame)
            .unwrap()
            .into_iter()
            .enumerate()
        {
            let f = crate::protocol::Frame {
                session: 7,
                kind: 4,
                channel: 4,
                opcode: 0x480,
                sequence: 1 + i as u32,
                payload,
            };
            if let Some(b) = receiver.feed(&f, std::time::Instant::now()).unwrap() {
                to.receive(b, now);
            }
        }
    }
}
fn dns_question() -> Vec<u8> {
    vec![
        0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 4, b't', b'e', b's', b't', 7, b'i', b'n', b'v',
        b'a', b'l', b'i', b'd', 0, 0, 1, 0, 1,
    ]
}
fn dns_reply() -> Vec<u8> {
    let mut reply = dns_question();
    reply[2] = 0x81;
    reply[3] = 0x83;
    reply
}
#[tokio::test]
async fn ipv4_target_dns_udp_reaches_owned_ipv6_resolver_and_keeps_virtual_source() {
    let socket = UdpSocket::bind("[::1]:0").await.unwrap();
    let addr = socket.local_addr().unwrap();
    let response = tokio::spawn(async move {
        let mut bytes = [0u8; 1500];
        let (n, source) = socket.recv_from(&mut bytes).await.unwrap();
        assert!(source.is_ipv6());
        assert_eq!(&bytes[..n], dns_question());
        socket.send_to(&dns_reply(), source).await.unwrap();
    });
    let (mut server, mut peer) = pair(Host {
        dns: addr,
        dial: Arc::new(crate::network::host_io::NativeDial),
    });
    let mut socket = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 4096]),
    );
    socket.bind(40053).unwrap();
    let endpoint = IpEndpoint::new(Ipv4Addr::new(192, 168, 127, 1).into(), 53);
    socket.send_slice(&dns_question(), endpoint).unwrap();
    let handle = peer.sockets.add(socket);
    let mut id = 0;
    let mut received = false;
    for now in 0..3000 {
        peer.poll(now);
        pump(&mut peer, &mut server, now, &mut id);
        server.poll(now);
        pump(&mut server, &mut peer, now, &mut id);
        let socket = peer.sockets.get_mut::<udp::Socket>(handle);
        if socket.can_recv() {
            let (bytes, meta) = socket.recv().unwrap();
            assert_eq!(bytes, dns_reply());
            assert_eq!(meta.endpoint, endpoint);
            received = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(
        received,
        "IPv6 DNS UDP reply did not reach the IPv4 NCM client"
    );
    response.await.unwrap();
    server.shutdown_io().await;
}
#[tokio::test]
async fn ipv4_target_dns_tcp_preserves_length_prefix_and_half_close_over_ipv6() {
    let listener = TcpListener::bind("[::1]:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let query = dns_question();
    let reply = dns_reply();
    let mut request = (query.len() as u16).to_be_bytes().to_vec();
    request.extend_from_slice(&query);
    let mut expected = (reply.len() as u16).to_be_bytes().to_vec();
    expected.extend_from_slice(&reply);
    let captured = request.clone();
    let response = expected.clone();
    let responder = tokio::spawn(async move {
        let (mut socket, source) = listener.accept().await.unwrap();
        assert!(source.is_ipv6());
        let mut got = vec![];
        socket.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, captured);
        socket.write_all(&response).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let (mut server, mut peer) = pair(Host {
        dns: addr,
        dial: Arc::new(crate::network::host_io::NativeDial),
    });
    let mut socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 16384]),
        tcp::SocketBuffer::new(vec![0; 16384]),
    );
    socket
        .connect(
            peer.interface.context(),
            (Ipv4Addr::new(192, 168, 127, 1), 53),
            40053,
        )
        .unwrap();
    let handle = peer.sockets.add(socket);
    let mut id = 0;
    let mut sent = 0;
    let mut fin = false;
    let mut got = vec![];
    for now in 0..4000 {
        let socket = peer.sockets.get_mut::<tcp::Socket>(handle);
        if socket.can_send() && sent < request.len() {
            sent += socket.send_slice(&request[sent..]).unwrap();
        }
        if sent == request.len() && !fin {
            socket.close();
            fin = true;
        }
        peer.poll(now);
        pump(&mut peer, &mut server, now, &mut id);
        server.poll(now);
        pump(&mut server, &mut peer, now, &mut id);
        let socket = peer.sockets.get_mut::<tcp::Socket>(handle);
        if socket.can_recv() {
            socket
                .recv(|bytes| {
                    got.extend_from_slice(bytes);
                    (bytes.len(), ())
                })
                .unwrap();
        }
        if fin && got.len() == expected.len() && !socket.may_recv() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(sent, request.len());
    assert_eq!(got, expected);
    responder.await.unwrap();
    server.shutdown_io().await;
}
#[tokio::test]
async fn actual_host_udp_echo_and_dns_relay_over_ncm_keep_original_remote_endpoints() {
    let echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = echo.local_addr().unwrap();
    let responder = tokio::spawn(async move {
        let mut b = [0; 1473];
        for _ in 0..2 {
            let (n, peer) = echo.recv_from(&mut b).await.unwrap();
            echo.send_to(&b[..n], peer).await.unwrap();
        }
    });
    let (mut server, mut peer) = pair(Host {
        dns: addr,
        dial: Arc::new(Local(addr)),
    });
    let mut s = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 16000]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 16000]),
    );
    s.bind(40000).unwrap();
    let handle = peer.sockets.add(s);
    let mut id = 0;
    for (remote, port) in [
        (Ipv4Addr::new(8, 8, 4, 4), 12345),
        (Ipv4Addr::new(192, 168, 127, 1), 53),
    ] {
        let expected = vec![0xef; 1472];
        peer.sockets
            .get_mut::<udp::Socket>(handle)
            .send_slice(&expected, (remote, port))
            .unwrap();
        let mut found = false;
        for now in 0..3000 {
            peer.poll(now);
            pump(&mut peer, &mut server, now, &mut id);
            server.poll(now);
            pump(&mut server, &mut peer, now, &mut id);
            let s = peer.sockets.get_mut::<udp::Socket>(handle);
            if s.can_recv() {
                let (data, meta) = s.recv().unwrap();
                assert_eq!(data, expected);
                assert_eq!(meta.endpoint, IpEndpoint::new(remote.into(), port));
                found = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert!(found, "UDP/DNS echo did not arrive");
    }
    responder.await.unwrap();
    assert_eq!(server.flows(), 2);
    server.poll(31_000);
    server.poll(31_101);
    assert_eq!(server.flows(), 0);
}
#[tokio::test]
async fn actual_host_tcp_large_stream_and_target_half_close_survive_chunking_and_backpressure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let expected: Vec<_> = (0..65536).map(|n| (n % 251) as u8).collect();
    let fixture = expected.clone();
    let responder = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut b = vec![];
        socket.read_to_end(&mut b).await.unwrap();
        assert_eq!(b, fixture);
        socket.write_all(&b).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let (mut server, mut peer) = pair(Host {
        dns: addr,
        dial: Arc::new(Local(addr)),
    });
    let mut socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 16384]),
        tcp::SocketBuffer::new(vec![0; 16384]),
    );
    socket
        .connect(
            peer.interface.context(),
            (Ipv4Addr::new(8, 8, 4, 4), 8080),
            40000,
        )
        .unwrap();
    let handle = peer.sockets.add(socket);
    let mut id = 0;
    let mut sent = 0;
    let mut got = vec![];
    let mut fin = false;
    for now in 0..5000 {
        let s = peer.sockets.get_mut::<tcp::Socket>(handle);
        if s.can_send() && sent < expected.len() {
            sent += s.send_slice(&expected[sent..]).unwrap();
        }
        if sent == expected.len() && !fin {
            s.close();
            fin = true;
        }
        peer.poll(now);
        pump(&mut peer, &mut server, now, &mut id);
        server.poll(now);
        pump(&mut server, &mut peer, now, &mut id);
        let s = peer.sockets.get_mut::<tcp::Socket>(handle);
        while s.can_recv() {
            let mut b = [0; 4096];
            let n = s.recv_slice(&mut b).unwrap();
            got.extend_from_slice(&b[..n]);
        }
        if got.len() == expected.len() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(sent, expected.len());
    assert_eq!(got, expected);
    responder.await.unwrap();
    assert_eq!(server.dropped(), 0);
    // Completion may be observed for many polls: never poll a completed JoinHandle twice.
    for now in 5000..5100 {
        server.poll(now);
        pump(&mut server, &mut peer, now, &mut id);
        peer.poll(now);
        pump(&mut peer, &mut server, now, &mut id);
        tokio::task::yield_now().await;
    }
    server.poll(5200);
    assert_eq!(server.flows(), 0);
}
#[test]
fn rejected_frames_do_not_allocate_host_flows() {
    let mut gateway = Gateway::new(
        Config::default(),
        Host {
            dns: "127.0.0.1:53".parse().unwrap(),
            dial: Arc::new(super::super::host_io::NativeDial),
        },
    )
    .unwrap();
    let mut sockets = SocketSet::new(vec![]);
    let mut invalid = vec![0; 64];
    invalid[12..14].copy_from_slice(&[8, 0]);
    assert!(gateway.ingress(invalid, &mut sockets, 0).is_none());
    assert_eq!(gateway.count(), 0);
}

fn frame(protocol: u8, client: Ipv4Addr, remote: Ipv4Addr, port: u16) -> Vec<u8> {
    let mut b = vec![0; 14 + 20 + if protocol == 6 { 20 } else { 12 }];
    b[12..14].copy_from_slice(&[8, 0]);
    b[14] = 0x45;
    let len = (b.len() - 14) as u16;
    b[16..18].copy_from_slice(&len.to_be_bytes());
    b[22] = 64;
    b[23] = protocol;
    b[26..30].copy_from_slice(&client.octets());
    b[30..34].copy_from_slice(&remote.octets());
    b[34..36].copy_from_slice(&port.to_be_bytes());
    b[36..38].copy_from_slice(&8080u16.to_be_bytes());
    if protocol == 6 {
        b[46] = 0x50;
        b[47] = 2;
    } else {
        b[38..40].copy_from_slice(&12u16.to_be_bytes());
    }
    let c = crate::network::dhcp::checksum(&b[14..34]);
    b[24..26].copy_from_slice(&c.to_be_bytes());
    ipv4::packet(&b)
        .unwrap()
        .rewrite(client, port, remote, 8080)
}
#[tokio::test]
async fn malformed_or_forbidden_traffic_opens_no_flow_and_tcp_udp_slots_are_bounded() {
    let host = Host {
        dns: "127.0.0.1:53".parse().unwrap(),
        dial: Arc::new(Local("127.0.0.1:9".parse().unwrap())),
    };
    let mut gateway = Gateway::new(Config::default(), host).unwrap();
    let mut sockets = SocketSet::new(vec![]);
    let client = Ipv4Addr::new(192, 168, 127, 100);
    let remote = Ipv4Addr::new(8, 8, 4, 4);
    let valid = frame(17, client, remote, 40000);
    let mut cases = vec![
        frame(17, client, Ipv4Addr::LOCALHOST, 40000),
        frame(17, Ipv4Addr::new(192, 168, 126, 100), remote, 40000),
    ];
    let mut corrupt = valid.clone();
    corrupt[44] ^= 1;
    cases.push(corrupt);
    let mut ttl = valid.clone();
    ttl[22] = 1;
    ttl[24..26].fill(0);
    let c = crate::network::dhcp::checksum(&ttl[14..34]);
    ttl[24..26].copy_from_slice(&c.to_be_bytes());
    cases.push(ttl);
    let mut frag = valid.clone();
    frag[20] = 0x20;
    frag[24..26].fill(0);
    let c = crate::network::dhcp::checksum(&frag[14..34]);
    frag[24..26].copy_from_slice(&c.to_be_bytes());
    cases.push(frag);
    let mut ack = frame(6, client, remote, 40000);
    ack[47] = 0x10;
    let p = ipv4::packet(&ack).unwrap();
    cases.push(p.rewrite(client, 40000, remote, 8080));
    for b in cases {
        assert!(gateway.ingress(b, &mut sockets, 0).is_none());
    }
    assert_eq!(gateway.count(), 0);
    for i in 0..24 {
        assert!(gateway
            .ingress(frame(17, client, remote, 40000 + i), &mut sockets, 0)
            .is_some());
    }
    assert!(gateway
        .ingress(frame(17, client, remote, 40024), &mut sockets, 0)
        .is_none());
    for i in 0..8 {
        assert!(gateway
            .ingress(frame(6, client, remote, 41000 + i), &mut sockets, 0)
            .is_some());
    }
    assert!(gateway
        .ingress(frame(6, client, remote, 41008), &mut sockets, 0)
        .is_none());
    assert_eq!(gateway.count(), 32);
    gateway.poll(&mut sockets, 121_000);
    gateway.poll_queued(&mut sockets, 121_101, false);
    assert_eq!(
        gateway.count(),
        32,
        "translated RX backlog must drain before slot reuse"
    );
    gateway.poll(&mut sockets, 121_101);
    assert_eq!(gateway.count(), 0);
    // A released slot can be used again without creating extra permanent listeners.
    assert!(gateway
        .ingress(frame(6, client, remote, 42000), &mut sockets, 121_102)
        .is_some());
    assert_eq!(gateway.count(), 1);
}

#[test]
fn rewrite_preserves_payload_and_repairs_both_checksums_with_padding() {
    let source = Ipv4Addr::new(192, 168, 127, 100);
    let remote = Ipv4Addr::new(8, 8, 4, 4);
    for protocol in [6, 17] {
        let mut b = frame(protocol, source, remote, 40000);
        let original = b.len();
        b.extend_from_slice(&[0; 18]);
        let p = ipv4::packet(&b).unwrap();
        assert!(p.valid());
        let mapped = p.rewrite(source, 40000, Ipv4Addr::new(192, 168, 127, 1), 20000);
        assert_eq!(mapped.len(), original);
        let view = ipv4::packet(&mapped).unwrap();
        assert!(view.valid());
        let roundtrip = view.rewrite(source, 40000, remote, 8080);
        assert_eq!(roundtrip, &b[..original]);
        let mut bad = mapped;
        bad[34] ^= 1;
        assert!(!ipv4::packet(&bad).unwrap().valid());
    }
}
