use std::net::{IpAddr, SocketAddr};

pub fn serialize_socket_address(addr: SocketAddr) -> [u8; 6] {
    match addr.ip() {
        IpAddr::V4(ip) => {
            let [a, b, c, d] = ip.octets();
            let [p0, p1] = addr.port().to_be_bytes();
            [a, b, c, d, p0, p1]
        }
        _ => panic!("only ipv4 is supported"),
    }
}
