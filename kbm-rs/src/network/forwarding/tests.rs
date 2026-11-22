use super::*;
use crate::network::{
    framing,
    gateway::Gateway,
    host_io::{Host, NativeDial},
    stack::Engine,
};
use smoltcp::wire::{IpCidr, Ipv4Cidr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
fn owner() -> Principal {
    Principal {
        key_id: "app-network".into(),
        client_id: "app-network".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    }
}
fn rule(protocol: Protocol, port: u16) -> Rule {
    Rule {
        id: format!("rule-{port}"),
        protocol,
        listen_port: port,
        target_ip: "192.168.127.100".into(),
        target_port: 8080,
    }
}
async fn bound(protocol: Protocol) -> Bound {
    let (port, listener) = match protocol {
        Protocol::Tcp => {
            let s = TcpListener::bind("127.0.0.1:0").await.unwrap();
            (s.local_addr().unwrap().port(), Listener::Tcp(s))
        }
        Protocol::Udp => {
            let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            (s.local_addr().unwrap().port(), Listener::Udp(s))
        }
    };
    Bound {
        rules: vec![rule(protocol, port)],
        listeners: vec![listener],
    }
}
fn pair() -> (Engine, Engine) {
    let server = Engine::new(Config::default(), [2, 0x4b, 0x42, 0x4d, 0, 1], false).unwrap();
    let mut peer = Engine::new(Config::default(), [2, 17, 34, 51, 68, 85], false).unwrap();
    peer.interface.update_ip_addrs(|ips| {
        ips.clear();
        ips.push(IpCidr::Ipv4(Ipv4Cidr::new(
            Ipv4Addr::new(192, 168, 127, 100),
            24,
        )))
        .unwrap();
    });
    (server, peer)
}
fn pump(from: &mut Engine, to: &mut Engine, now: u64, id: &mut u32) {
    while let Some(frame) = from.output() {
        *id += 1;
        let mut rx = framing::Receiver::synchronized(7, 0, framing::WINDOW).unwrap();
        for (sequence, payload) in framing::fragments(*id, &frame)
            .unwrap()
            .into_iter()
            .enumerate()
        {
            let frame = crate::protocol::Frame {
                session: 7,
                kind: 4,
                channel: 4,
                opcode: 0x480,
                sequence: sequence as u32 + 1,
                payload,
            };
            if let Some(b) = rx.feed(&frame, std::time::Instant::now()).unwrap() {
                to.receive(b, now);
            }
        }
    }
}
#[test]
fn rules_reject_ambiguous_binds_and_other_subnets() {
    let rule = Rule {
        id: "ssh".into(),
        protocol: Protocol::Tcp,
        listen_port: 12345,
        target_ip: "192.168.127.100".into(),
        target_port: 22,
    };
    assert!(validate(std::slice::from_ref(&rule), &Config::default()).is_ok());
    assert!(validate(&[rule.clone(), rule.clone()], &Config::default()).is_err());
    assert!(validate(
        &[Rule {
            target_ip: "127.0.0.1".into(),
            ..rule.clone()
        }],
        &Config::default()
    )
    .is_err());
    assert!(validate(
        &[Rule {
            listen_port: 0,
            ..rule
        }],
        &Config::default()
    )
    .is_err());
}

#[tokio::test]
async fn actual_loopback_tcp_forwards_64k_and_half_close_over_ncm_then_releases_the_port() {
    let bound = bound(Protocol::Tcp).await;
    let port = bound.rules[0].listen_port;
    let (mut server, mut peer) = pair();
    let mut listener = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 16384]),
        tcp::SocketBuffer::new(vec![0; 16384]),
    );
    listener.listen(8080).unwrap();
    let target = peer.sockets.add(listener);
    server
        .start_forward(bound, owner(), CancellationToken::new())
        .unwrap();
    let expected: Vec<u8> = (0..65536).map(|n| (n % 251) as u8).collect();
    let payload = expected.clone();
    let client = tokio::spawn(async move {
        let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        socket.write_all(&payload).await.unwrap();
        socket.shutdown().await.unwrap();
        let mut data = vec![];
        socket.read_to_end(&mut data).await.unwrap();
        data
    });
    let began = std::time::Instant::now();
    let mut id = 0;
    while !client.is_finished() && began.elapsed().as_secs() < 10 {
        let now = began.elapsed().as_millis() as u64;
        server.poll(now);
        pump(&mut server, &mut peer, now, &mut id);
        let s = peer.sockets.get_mut::<tcp::Socket>(target);
        while s.can_recv() && s.can_send() {
            let count = (s.send_capacity() - s.send_queue()).min(4096);
            if count == 0 {
                break;
            }
            let mut b = vec![0; count];
            let n = s.recv_slice(&mut b).unwrap();
            assert_eq!(s.send_slice(&b[..n]).unwrap(), n);
        }
        if !s.may_recv() && [tcp::State::Established, tcp::State::CloseWait].contains(&s.state()) {
            s.close();
        }
        peer.poll(now);
        pump(&mut peer, &mut server, now, &mut id);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert!(client.is_finished(), "forwarded TCP stalled");
    assert_eq!(client.await.unwrap(), expected);
    assert!(id > 64);
    server.stop_forward().finish().await;
    assert!(!server.forward_status().running);
    assert_eq!(server.flow_usage(), 0);
    let rebound = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    drop(rebound);
}

#[tokio::test]
async fn udp_clients_remain_separate_late_replies_are_rejected_and_stop_releases_the_socket() {
    let bound = bound(Protocol::Udp).await;
    let port = bound.rules[0].listen_port;
    let (mut server, mut peer) = pair();
    let mut socket = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 12000]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 12000]),
    );
    socket.bind(8080).unwrap();
    let target = peer.sockets.add(socket);
    server
        .start_forward(bound, owner(), CancellationToken::new())
        .unwrap();
    let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    a.send_to(&vec![0xef; 1472], (Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    b.send_to(b"second-client", (Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    let began = std::time::Instant::now();
    let mut id = 0;
    let mut received = 0;
    let mut old_port = None;
    let mut a_data = None;
    let mut b_data = None;
    let mut buffer = [0; 1473];
    while (a_data.is_none() || b_data.is_none()) && began.elapsed().as_secs() < 5 {
        let now = began.elapsed().as_millis() as u64;
        server.poll(now);
        pump(&mut server, &mut peer, now, &mut id);
        let s = peer.sockets.get_mut::<udp::Socket>(target);
        while s.can_recv() {
            let (data, meta) = s.recv().unwrap();
            let data = data.to_vec();
            old_port = Some(meta.endpoint.port);
            s.send_slice(&data, meta.endpoint).unwrap();
            received += 1;
        }
        peer.poll(now);
        pump(&mut peer, &mut server, now, &mut id);
        if a_data.is_none() {
            if let Ok(n) = a.try_recv(&mut buffer) {
                a_data = Some(buffer[..n].to_vec());
            }
        }
        if b_data.is_none() {
            if let Ok(n) = b.try_recv(&mut buffer) {
                b_data = Some(buffer[..n].to_vec());
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(received, 2);
    assert_eq!(a_data.unwrap(), vec![0xef; 1472]);
    assert_eq!(b_data.unwrap(), b"second-client");
    let scopes = server
        .forward
        .flows
        .iter()
        .map(|flow| flow.scope.clone())
        .collect::<Vec<_>>();
    server.poll(31_000);
    assert_eq!(server.forward_status().connections, 0);
    assert!(
        server.forward_status().error.is_none(),
        "idle UDP expiration is normal cleanup"
    );
    assert!(scopes.iter().all(Scope::cancelled));
    let late = peer.sockets.get_mut::<udp::Socket>(target);
    late.send_slice(
        b"late",
        (Ipv4Addr::new(192, 168, 127, 1), old_port.unwrap()),
    )
    .unwrap();
    peer.poll(31_001);
    pump(&mut peer, &mut server, 31_001, &mut id);
    assert!(server.dropped() > 0);
    server.stop_forward().finish().await;
    assert_eq!(server.flow_usage(), 0);
    let rebound = UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).await.unwrap();
    drop(rebound);
}

#[tokio::test]
async fn accepted_and_gateway_flows_share_one_32_connection_budget() {
    let budget = Budget::default();
    let mut held = (0..31)
        .map(|_| budget.reserve().unwrap())
        .collect::<Vec<_>>();
    let bound = bound(Protocol::Tcp).await;
    let port = bound.rules[0].listen_port;
    let mut forward = Forwarder::new(Config::default(), budget.clone()).unwrap();
    forward
        .start(bound, owner(), CancellationToken::new())
        .unwrap();
    let local = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while budget.used() != 32 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let host = Host {
        dns: "127.0.0.1:53".parse().unwrap(),
        dial: Arc::new(NativeDial),
    };
    let mut gateway = Gateway::budgeted(
        Config::default(),
        host,
        CancellationToken::new(),
        budget.clone(),
    )
    .unwrap();
    let mut sockets = SocketSet::new(vec![]);
    let frame = ipv4::tcp_frame(
        [2, 17, 34, 51, 68, 85],
        [2, 0x4b, 0x42, 0x4d, 0, 1],
        ipv4::Key {
            protocol: 6,
            client: Ipv4Addr::new(192, 168, 127, 100),
            client_port: 40000,
            remote: Ipv4Addr::new(8, 8, 4, 4),
            remote_port: 80,
        },
        1,
        0,
        2,
    );
    assert!(gateway.ingress(frame.clone(), &mut sockets, 0).is_none());
    assert_eq!(gateway.count(), 0);
    held.pop();
    assert!(gateway.ingress(frame, &mut sockets, 0).is_some());
    assert_eq!(budget.used(), 32);
    forward.stop(&mut sockets).finish().await;
    drop(local);
    assert_eq!(budget.used(), 31);
    drop(gateway);
    drop(held);
    assert_eq!(budget.used(), 0);
}

#[tokio::test]
async fn bind_failure_rolls_back_new_ports_without_closing_the_existing_listener() {
    let old = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let occupied = old.local_addr().unwrap().port();
    let available = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let free = available.local_addr().unwrap().port();
    drop(available);
    let rules = vec![rule(Protocol::Tcp, free), rule(Protocol::Tcp, occupied)];
    let error = Bound::bind(rules, &Config::default()).await.err().unwrap();
    assert_eq!(error.code, "FORWARD_BIND_FAILED");
    assert!(error.message.contains(&occupied.to_string()));
    let rebound = TcpListener::bind((Ipv4Addr::LOCALHOST, free))
        .await
        .unwrap();
    drop(rebound);
    let client = TcpStream::connect((Ipv4Addr::LOCALHOST, occupied))
        .await
        .unwrap();
    let (accepted, _) = old.accept().await.unwrap();
    drop((client, accepted));
}

#[tokio::test]
async fn udp_client_queue_is_eight_datagrams_and_pending_clients_are_reaped_on_stop() {
    let budget = Budget::default();
    let bound = bound(Protocol::Udp).await;
    let port = bound.rules[0].listen_port;
    let mut forward = Forwarder::new(Config::default(), budget.clone()).unwrap();
    forward
        .start(bound, owner(), CancellationToken::new())
        .unwrap();
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    for _ in 0..64 {
        client
            .send_to(&[7; 32], (Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while forward.status().dropped == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let incoming = forward.runtime.as_mut().unwrap().events.try_recv().unwrap();
    assert_eq!(incoming.pipe.rx.len(), 8);
    assert_eq!(budget.used(), 1);
    drop(incoming);
    forward.stop(&mut SocketSet::new(vec![])).finish().await;
    assert_eq!(budget.used(), 0);
    UdpSocket::bind((Ipv4Addr::LOCALHOST, port)).await.unwrap();
}
