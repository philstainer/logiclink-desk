use rand::RngCore;
use serde::Serialize;
use thiserror::Error;

use crate::response::parse_response;

pub const GATT_SERVICE: &str = "b9934c435c91462b80a130fccc29d758";
pub const APP_CHARACTERISTIC: &str = "b9934c445c91462b80a130fccc29d758";
pub const EXTRA_CHARACTERISTIC: &str = "b9934c455c91462b80a130fccc29d758";

const XXTEA_KEY: [u8; 16] = [
    0x88, 0xd5, 0x3c, 0x1e, 0x02, 0x7c, 0x06, 0x1d, 0x4a, 0x84, 0x25, 0x3b, 0x1a, 0x18, 0x32, 0x4a,
];

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("unknown characteristic selector: {0}")]
    UnknownCharacteristic(String),
    #[error("nonce must be exactly 2 bytes")]
    InvalidNonce,
    #[error("packet too short")]
    PacketTooShort,
    #[error("decrypted packet too short: {len} bytes")]
    DecryptedPacketTooShort { len: usize },
    #[error("encrypted length mismatch: header={header} actual={actual}")]
    EncryptedLengthMismatch { header: usize, actual: usize },
    #[error("payload length exceeds decrypted packet: payload={payload_len} plain={plain_len}")]
    PayloadLengthMismatch {
        payload_len: usize,
        plain_len: usize,
    },
    #[error("CRC mismatch: header=0x{header:04x} actual=0x{actual:04x}")]
    CrcMismatch { header: u16, actual: u16 },
}

#[derive(Debug, Clone, Serialize)]
pub struct Packet {
    pub command: u8,
    pub status: u8,
    pub nonce: [u8; 2],
    #[serde(serialize_with = "crate::response::as_hex")]
    pub payload: Vec<u8>,
    #[serde(serialize_with = "crate::response::as_hex")]
    pub plain: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecodeFrame {
    #[serde(rename = "rawPacket", serialize_with = "crate::response::as_hex")]
    pub raw_packet: Vec<u8>,
    pub command: u8,
    #[serde(rename = "commandHex")]
    pub command_hex: String,
    #[serde(rename = "commandName")]
    pub command_name: String,
    pub status: u8,
    pub nonce: String,
    #[serde(serialize_with = "crate::response::as_hex")]
    pub payload: Vec<u8>,
    pub parsed: serde_json::Value,
    #[serde(serialize_with = "crate::response::as_hex")]
    pub plain: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecodeStream {
    pub frames: Vec<DecodeFrame>,
    #[serde(serialize_with = "crate::response::as_hex")]
    pub remainder: Vec<u8>,
}

pub fn resolve_characteristic(selector: &str) -> Result<&'static str, ProtocolError> {
    let normalized = selector.to_ascii_lowercase();
    match normalized.as_str() {
        "app" | "c44" | APP_CHARACTERISTIC => Ok(APP_CHARACTERISTIC),
        "extra" | "c45" | EXTRA_CHARACTERISTIC => Ok(EXTRA_CHARACTERISTIC),
        value if value.len() == 32 && value.chars().all(|ch| ch.is_ascii_hexdigit()) => {
            Ok(Box::leak(value.to_string().into_boxed_str()))
        }
        _ => Err(ProtocolError::UnknownCharacteristic(selector.to_string())),
    }
}

pub fn encode_packet(command: u8, payload: &[u8], nonce: Option<[u8; 2]>) -> Vec<u8> {
    let nonce = nonce.unwrap_or_else(|| {
        let mut bytes = [0_u8; 2];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    });
    let mut plain = Vec::with_capacity(4 + payload.len());
    plain.extend_from_slice(&[command, 0x00, nonce[0], nonce[1]]);
    plain.extend_from_slice(payload);

    let encrypted = xxtea_encrypt(&plain);
    let crc = crc16_ccitt_false(&encrypted);

    let mut out = Vec::with_capacity(4 + encrypted.len());
    out.push((encrypted.len() - 4) as u8);
    out.push(payload.len() as u8);
    out.push((crc & 0xff) as u8);
    out.push((crc >> 8) as u8);
    out.extend_from_slice(&encrypted);
    out
}

pub fn decode_packet(raw_packet: &[u8]) -> Result<Packet, ProtocolError> {
    if raw_packet.len() < 8 {
        return Err(ProtocolError::PacketTooShort);
    }
    let encrypted_len = raw_packet[0] as usize + 4;
    let payload_len = raw_packet[1] as usize;
    let crc = u16::from_le_bytes([raw_packet[2], raw_packet[3]]);
    let encrypted = &raw_packet[4..];

    if encrypted.len() != encrypted_len {
        return Err(ProtocolError::EncryptedLengthMismatch {
            header: encrypted_len,
            actual: encrypted.len(),
        });
    }

    let actual_crc = crc16_ccitt_false(encrypted);
    if actual_crc != crc {
        return Err(ProtocolError::CrcMismatch {
            header: crc,
            actual: actual_crc,
        });
    }

    let plain = xxtea_decrypt(encrypted);
    if plain.len() < 4 {
        return Err(ProtocolError::DecryptedPacketTooShort { len: plain.len() });
    }
    let required_plain_len = 4 + payload_len;
    if plain.len() < required_plain_len {
        return Err(ProtocolError::PayloadLengthMismatch {
            payload_len,
            plain_len: plain.len(),
        });
    }

    Ok(Packet {
        command: plain[0],
        status: plain[1],
        nonce: [plain[2], plain[3]],
        payload: plain[4..required_plain_len].to_vec(),
        plain,
    })
}

pub fn slip_encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.push(0xc0);
    for &byte in bytes {
        match byte {
            0xc0 => out.extend_from_slice(&[0xdb, 0xdc]),
            0xdb => out.extend_from_slice(&[0xdb, 0xdd]),
            _ => out.push(byte),
        }
    }
    out.push(0xc0);
    out
}

pub fn slip_decode(frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut escaped = false;
    for &byte in frame {
        if byte == 0xc0 {
            continue;
        }
        if escaped {
            match byte {
                0xdc => out.push(0xc0),
                0xdd => out.push(0xdb),
                _ => out.push(byte),
            }
            escaped = false;
        } else if byte == 0xdb {
            escaped = true;
        } else {
            out.push(byte);
        }
    }
    out
}

pub fn extract_slip_frames(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut frames = Vec::new();
    let mut current = Vec::new();
    let mut in_frame = false;

    for &byte in bytes {
        if byte == 0xc0 {
            if in_frame && !current.is_empty() {
                frames.push(slip_decode(&current));
            }
            current.clear();
            in_frame = true;
            continue;
        }
        if in_frame {
            current.push(byte);
        }
    }

    (frames, current)
}

pub fn decode_slip_stream(bytes: &[u8]) -> Result<DecodeStream, ProtocolError> {
    let (frames, remainder) = extract_slip_frames(bytes);
    let mut decoded = Vec::with_capacity(frames.len());
    for raw_packet in frames {
        let packet = decode_packet(&raw_packet)?;
        decoded.push(DecodeFrame {
            raw_packet,
            command: packet.command,
            command_hex: format!("0x{:02x}", packet.command),
            command_name: command_name(packet.command).to_string(),
            status: packet.status,
            nonce: hex::encode(packet.nonce),
            payload: packet.payload.clone(),
            parsed: parse_response(packet.command, &packet.payload),
            plain: packet.plain,
        });
    }
    Ok(DecodeStream {
        frames: decoded,
        remainder,
    })
}

pub fn crc16_ccitt_false(bytes: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for &byte in bytes {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn to_u32_words_le(bytes: &[u8]) -> Vec<u32> {
    let padded_len = bytes.len().div_ceil(4) * 4;
    let mut padded = vec![0_u8; padded_len];
    padded[..bytes.len()].copy_from_slice(bytes);
    padded
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("exact chunk")))
        .collect()
}

fn from_u32_words_le(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(words.len() * 4);
    for word in words {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out
}

pub fn xxtea_encrypt(bytes: &[u8]) -> Vec<u8> {
    let mut v = to_u32_words_le(bytes);
    let k = to_u32_words_le(&XXTEA_KEY);
    let n = v.len();
    if n < 2 {
        return from_u32_words_le(&v);
    }

    let mut z = v[n - 1];
    let mut sum = 0_u32;
    let delta = 0x9e3779b9_u32;
    let mut q = 6 + 52 / n as u32;

    while q > 0 {
        q -= 1;
        sum = sum.wrapping_add(delta);
        let e = (sum >> 2) & 3;
        for p in 0..n - 1 {
            let y = v[p + 1];
            let mx = (((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4)))
                ^ ((sum ^ y).wrapping_add(k[((p as u32 & 3) ^ e) as usize] ^ z));
            v[p] = v[p].wrapping_add(mx);
            z = v[p];
        }
        let y = v[0];
        let p = n - 1;
        let mx = (((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4)))
            ^ ((sum ^ y).wrapping_add(k[(((p as u32) & 3) ^ e) as usize] ^ z));
        v[p] = v[p].wrapping_add(mx);
        z = v[p];
    }

    from_u32_words_le(&v)
}

pub fn xxtea_decrypt(bytes: &[u8]) -> Vec<u8> {
    let mut v = to_u32_words_le(bytes);
    let k = to_u32_words_le(&XXTEA_KEY);
    let n = v.len();
    if n < 2 {
        return from_u32_words_le(&v);
    }

    let delta = 0x9e3779b9_u32;
    let q = 6 + 52 / n as u32;
    let mut sum = q.wrapping_mul(delta);

    while sum != 0 {
        let e = (sum >> 2) & 3;
        let mut y = v[0];
        for p in (1..n).rev() {
            let z = v[p - 1];
            let mx = (((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4)))
                ^ ((sum ^ y).wrapping_add(k[((p as u32 & 3) ^ e) as usize] ^ z));
            v[p] = v[p].wrapping_sub(mx);
            y = v[p];
        }
        let z = v[n - 1];
        let mx = (((z >> 5) ^ (y << 2)).wrapping_add((y >> 3) ^ (z << 4)))
            ^ ((sum ^ y).wrapping_add(k[e as usize] ^ z));
        v[0] = v[0].wrapping_sub(mx);
        sum = sum.wrapping_sub(delta);
    }

    from_u32_words_le(&v)
}

pub fn command_name(command: u8) -> &'static str {
    match command {
        0x08 => "height/motion",
        0x09 => "occupancy",
        0x0a => "led",
        0x11 => "ble-gadget",
        0x19 => "handset",
        0x1a => "buttons-bind",
        0x1b => "led-bind",
        0x1c => "serial-number",
        0x1d => "firmware-id",
        0x1f => "dispatch-data",
        0x21 => "table-stats",
        0x22 => "interface-stats",
        0x27 => "port-input",
        0x2b => "connection-interface",
        0x30 => "handset-protocol",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_packet_exposes_nonce() {
        let packet = encode_packet(0x08, &[0x00, 0x00, 0x00], Some([0x12, 0x34]));

        let decoded = decode_packet(&packet).unwrap();

        assert_eq!(decoded.command, 0x08);
        assert_eq!(decoded.nonce, [0x12, 0x34]);
        assert_eq!(decoded.payload, [0x00, 0x00, 0x00]);
    }

    #[test]
    fn decode_packet_rejects_payload_length_beyond_decrypted_plaintext() {
        let mut packet = encode_packet(0x08, &[0x00, 0x00, 0x00], Some([0x12, 0x34]));
        packet[1] = 250;

        let error = decode_packet(&packet).unwrap_err();

        assert!(matches!(
            error,
            ProtocolError::PayloadLengthMismatch {
                payload_len: 250,
                ..
            }
        ));
    }
}
