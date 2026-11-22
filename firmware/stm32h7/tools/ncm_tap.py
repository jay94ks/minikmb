"""Attach MiniKMB Ethernet frames to a dedicated existing Linux TAP interface."""
import argparse
import fcntl
import os
import select
import socket
import struct

from network_link import NetworkLink
from probe_management import Client, select_port


def bridge(serial_port, uid, interface):
    import serial

    socket.if_nametoindex(interface)
    name = interface.encode("ascii")
    if not 1 <= len(name) < 16:
        raise ValueError("invalid TAP interface name")
    # TUNSETIFF with IFF_TAP | IFF_NO_PI. The caller provisions and owns the interface.
    tap = os.open("/dev/net/tun", os.O_RDWR | os.O_NONBLOCK)
    try:
        fcntl.ioctl(tap, 0x400454CA, struct.pack("16sH22x", name, 0x1002))
        with serial.Serial(serial_port, timeout=0.003, write_timeout=1) as port:
            client = Client(port, None)
            hello = client.hello(uid)
            if not struct.unpack_from("<I", hello, 28)[0] & 0x20:
                raise RuntimeError("firmware does not offer an NCM bridge")
            def deliver(frame):
                try:
                    if os.write(tap, frame) != len(frame):
                        raise OSError("short write to TAP")
                except BlockingIOError:
                    return False
                return True

            link = NetworkLink(client, on_frame=deliver)
            link.enable()
            tx_dropped = 0
            try:
                while True:
                    # Wake for either transport; the timeout also services the lease.
                    # NetworkLink.poll is nonblocking, so an idle bridge must wait here.
                    ready, _, _ = select.select([tap, port], [], [], 0.05)
                    link.poll()
                    if tap in ready:
                        frame = os.read(tap, 2048)
                        if not 14 <= len(frame) <= 1518 or not link.send(frame):
                            tx_dropped += 1
            finally:
                print("TAP TX dropped", tx_dropped, "TAP RX dropped", link.rx_dropped)
                link.disable()
    finally:
        os.close(tap)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--uid", required=True, help="MiniKMB controller UID")
    parser.add_argument("--tap", required=True, help="dedicated pre-configured TAP")
    args = parser.parse_args()
    uid = args.uid.upper()
    if len(uid) != 24 or any(char not in "0123456789ABCDEF" for char in uid):
        parser.error("expected 24 hex digits for --uid")
    try:
        bridge(select_port(uid, (0xCAFE, 0x4001)), uid, args.tap)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
