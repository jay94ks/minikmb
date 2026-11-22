use super::*;
use crate::{network::framing, protocol::Frame};
use smoltcp::{
    socket::{tcp, udp},
    wire::IpAddress,
};
use std::net::Ipv4Addr;
fn engines() -> (Engine, Engine) {
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
struct Link {
    id: u32,
    seq: u32,
    rx: framing::Receiver,
}
impl Link {
    fn new() -> Self {
        Self {
            id: 0,
            seq: 0,
            rx: framing::Receiver::synchronized(7, 0, framing::WINDOW).unwrap(),
        }
    }
    fn pump(&mut self, from: &mut Engine, to: &mut Engine, now: u64) {
        while let Some(frame) = from.output() {
            self.id += 1;
            let parts = framing::fragments(self.id, &frame).unwrap();
            for payload in parts {
                self.seq += 1;
                let event = Frame {
                    session: 7,
                    kind: 4,
                    channel: 4,
                    opcode: 0x480,
                    sequence: self.seq,
                    payload,
                };
                if let Some(b) = self.rx.feed(&event, std::time::Instant::now()).unwrap() {
                    to.receive(b, now);
                }
                self.rx.renewal().unwrap();
            }
        }
    }
}
#[test]
fn actual_smoltcp_tcp_stream_roundtrip_over_fragmented_management_frames() {
    let (mut server, mut peer) = engines();
    let mut socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 16384]),
        tcp::SocketBuffer::new(vec![0; 16384]),
    );
    socket.listen(4242).unwrap();
    let server_id = server.sockets.add(socket);
    let peer_id = peer.sockets.add(tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 16384]),
        tcp::SocketBuffer::new(vec![0; 16384]),
    ));
    peer.sockets
        .get_mut::<tcp::Socket>(peer_id)
        .connect(
            peer.interface.context(),
            (Ipv4Addr::new(192, 168, 127, 1), 4242),
            40000,
        )
        .unwrap();
    let expected: Vec<_> = (0..65536).map(|n| (n % 251) as u8).collect();
    let mut sent = 0;
    let mut received = Vec::new();
    let mut a = Link::new();
    let mut b = Link::new();
    for now in 0..5000 {
        let socket = peer.sockets.get_mut::<tcp::Socket>(peer_id);
        if socket.can_send() && sent < expected.len() {
            sent += socket.send_slice(&expected[sent..]).unwrap();
        }
        peer.poll(now);
        a.pump(&mut peer, &mut server, now);
        let socket = server.sockets.get_mut::<tcp::Socket>(server_id);
        if socket.can_recv() && socket.can_send() {
            let count = socket
                .send_capacity()
                .saturating_sub(socket.send_queue())
                .min(4096);
            if count > 0 {
                let mut data = vec![0; count];
                let n = socket.recv_slice(&mut data).unwrap();
                assert_eq!(socket.send_slice(&data[..n]).unwrap(), n);
            }
        }
        server.poll(now);
        b.pump(&mut server, &mut peer, now);
        let socket = peer.sockets.get_mut::<tcp::Socket>(peer_id);
        while socket.can_recv() {
            let mut data = [0; 4096];
            let n = socket.recv_slice(&mut data).unwrap();
            received.extend_from_slice(&data[..n]);
        }
        if received.len() == expected.len() {
            break;
        }
    }
    assert_eq!(sent, expected.len());
    assert_eq!(received, expected);
    assert!(a.seq > a.id);
    assert!(b.seq > b.id);
    assert_eq!(server.dropped(), 0);
    assert_eq!(peer.dropped(), 0);
}
fn udp_socket() -> udp::Socket<'static> {
    udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 12000]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 8], vec![0; 12000]),
    )
}
#[test]
fn actual_smoltcp_udp_max_datagram_roundtrip_and_bounded_device_queues() {
    let (mut server, mut peer) = engines();
    let mut s = udp_socket();
    s.bind(4242).unwrap();
    let server_id = server.sockets.add(s);
    let mut c = udp_socket();
    c.bind(40000).unwrap();
    let peer_id = peer.sockets.add(c);
    let expected = vec![0xef; 1472];
    peer.sockets
        .get_mut::<udp::Socket>(peer_id)
        .send_slice(
            &expected,
            (IpAddress::Ipv4(Ipv4Addr::new(192, 168, 127, 1)), 4242),
        )
        .unwrap();
    let mut a = Link::new();
    let mut b = Link::new();
    let mut received = None;
    for now in 0..1000 {
        peer.poll(now);
        a.pump(&mut peer, &mut server, now);
        let s = server.sockets.get_mut::<udp::Socket>(server_id);
        if s.can_recv() {
            let mut data = [0; 1500];
            let (n, meta) = s.recv_slice(&mut data).unwrap();
            s.send_slice(&data[..n], meta.endpoint).unwrap();
        }
        server.poll(now);
        b.pump(&mut server, &mut peer, now);
        let s = peer.sockets.get_mut::<udp::Socket>(peer_id);
        if s.can_recv() {
            let mut data = [0; 1500];
            let (n, _) = s.recv_slice(&mut data).unwrap();
            received = Some(data[..n].to_vec());
            break;
        }
    }
    assert_eq!(received.unwrap(), expected);
    let mut arp = vec![0; 42];
    arp[..6].fill(255);
    arp[6..12].copy_from_slice(&[2, 17, 34, 51, 68, 85]);
    arp[12..22].copy_from_slice(&[8, 6, 0, 1, 8, 0, 6, 4, 0, 1]);
    arp[22..28].copy_from_slice(&[2, 17, 34, 51, 68, 85]);
    arp[28..32].copy_from_slice(&[192, 168, 127, 100]);
    arp[38..42].copy_from_slice(&[192, 168, 127, 1]);
    for now in 1000..1100 {
        server.receive(arp.clone(), now);
    }
    assert!(server.device.tx.len() <= QUEUE);
    assert!(server.device.rx.len() <= QUEUE);
    assert!(server.dropped() > 0);
}
