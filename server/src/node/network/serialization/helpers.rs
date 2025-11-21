use crate::errors::AppError;
use std::net::{IpAddr, SocketAddr};

pub fn read_bytes<const N: usize>(
    payload: &[u8],
    range: std::ops::Range<usize>,
) -> Result<[u8; N], AppError> {
    payload
        .get(range.clone())
        .ok_or_else(|| AppError::InvalidData {
            details: format!("not enough bytes to deserialize operation: {:?}", range),
        })?
        .try_into()
        .map_err(|e| AppError::InvalidData {
            details: format!("not enough bytes to deserialize operation: {e}"),
        })
}

pub fn deserialize_socket_addr(bytes: [u8; 6]) -> Result<SocketAddr, AppError> {
    let ip_octets: [u8; 4] = read_bytes(&bytes, 0..4)?;
    let port_srl: [u8; 2] = read_bytes(&bytes, 4..6)?;
    let port = u16::from_be_bytes(port_srl);
    Ok(
        SocketAddr::try_from((ip_octets, port)).map_err(|e| AppError::InvalidProtocol {
            details: format!("could not socket address bytes {e}"),
        })?,
    )
}

pub fn serialize_socket_addr(addr: SocketAddr) -> Result<Vec<u8>, AppError> {
    let mut bytes = vec![];
    match addr.ip() {
        IpAddr::V4(ip) => {
            bytes.extend(ip.octets());
        }
        _ => {
            return Err(AppError::InvalidProtocol {
                details: "address is not IPv4".to_string(),
            })
        }
    }

    bytes.extend(addr.port().to_be_bytes());
    Ok(bytes)
}
