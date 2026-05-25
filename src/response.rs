use std::collections::BTreeMap;

use serde::Serializer;
use serde_json::{Value, json};

use crate::commands::{
    COMMAND_BLE_GADGET, COMMAND_BUTTONS_BIND, COMMAND_CONNECTION_INTERFACE, COMMAND_FIRMWARE_ID,
    COMMAND_HANDSET, COMMAND_HANDSET_PROTOCOL, COMMAND_HEIGHT, COMMAND_INTERFACE_STATS,
    COMMAND_LED, COMMAND_LED_BIND, COMMAND_OCCUPANCY, COMMAND_PORT_INPUT, COMMAND_SERIAL_NUMBER,
    COMMAND_TABLE_STATS,
};

pub fn as_hex<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&hex::encode(bytes))
}

pub fn parse_response(command: u8, payload: &[u8]) -> Value {
    if payload.is_empty() {
        return json!({ "result": null });
    }

    let result = payload[0];
    let mut base = BTreeMap::new();
    base.insert("result".to_string(), json!(result));
    base.insert(
        "resultDescription".to_string(),
        json!(result_description(result)),
    );

    match command {
        COMMAND_HEIGHT if payload.len() >= 7 => with_fields(
            base,
            &[
                ("kind", json!("height")),
                ("height", json!(u16_le(payload, 1))),
                ("movementCounter", json!(u32_le(payload, 3))),
            ],
        ),
        COMMAND_HEIGHT => with_fields(
            base,
            &[
                ("kind", json!("height-or-motion-ack")),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_OCCUPANCY if payload.len() >= 6 => with_fields(
            base,
            &[
                ("kind", json!("occupancy")),
                ("occupied", json!(payload[1] != 0)),
                ("occupationTime", json!(u32_le(payload, 2))),
            ],
        ),
        COMMAND_LED => with_fields(
            base,
            &[
                ("kind", json!("led-ack")),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_BUTTONS_BIND => with_fields(
            base,
            &[
                ("kind", json!("buttons-bind-ack")),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_LED_BIND => with_fields(
            base,
            &[
                ("kind", json!("led-bind-ack")),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_BLE_GADGET if payload.len() >= 4 => with_fields(
            base,
            &[
                ("kind", json!("ble-gadget")),
                ("family", json!(payload[1])),
                ("operation", json!(payload[2])),
                ("selector", json!(payload[3])),
                ("data", json!(hex::encode(&payload[4..]))),
            ],
        ),
        COMMAND_BLE_GADGET => with_fields(
            base,
            &[
                ("kind", json!("ble-gadget-ack")),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_FIRMWARE_ID if payload.len() >= 6 => with_fields(
            base,
            &[
                ("kind", json!("firmware-id")),
                ("version", json!(u32_le(payload, 1))),
                ("dirty", json!(payload[5] != 0)),
            ],
        ),
        COMMAND_SERIAL_NUMBER if payload.len() == 13 => with_fields(
            base,
            &[
                ("kind", json!("serial-number")),
                ("product", json!(u16_le(payload, 1))),
                ("variant", json!(u16_le(payload, 3))),
                ("serial", json!(u64_le(payload, 5).to_string())),
            ],
        ),
        COMMAND_PORT_INPUT if payload.len() >= 2 => {
            let count = payload[1] as usize;
            let values: Vec<u16> = (0..count)
                .filter_map(|index| {
                    let offset = 2 + index * 2;
                    (offset + 1 < payload.len()).then(|| u16_le(payload, offset))
                })
                .collect();
            let complete = values.len() == count;
            with_fields(
                base,
                &[
                    ("kind", json!("port-input")),
                    ("count", json!(count)),
                    ("values", json!(values)),
                    ("complete", json!(complete)),
                ],
            )
        }
        COMMAND_TABLE_STATS if payload.len() >= 26 => with_fields(
            base,
            &[
                ("kind", json!("table-stats")),
                (
                    "counters",
                    json!({
                        "at2": u32_le(payload, 2),
                        "at6": u32_le(payload, 6),
                        "at10": u32_le(payload, 10),
                        "at14": u32_le(payload, 14),
                        "at22": u32_le(payload, 22),
                    }),
                ),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_INTERFACE_STATS if payload.len() >= 69 => with_fields(
            base,
            &[
                ("kind", json!("interface-stats")),
                ("statusByteAt2", json!(payload[2])),
                (
                    "counters",
                    json!({
                        "at3": u32_le(payload, 3),
                        "at39": u32_le(payload, 39),
                        "at43": u32_le(payload, 43),
                        "at47": u32_le(payload, 47),
                        "at51": u32_le(payload, 51),
                        "at55": u32_le(payload, 55),
                    }),
                ),
                (
                    "byteWindows",
                    json!({
                        "at7": hex::encode(&payload[7..23]),
                        "at23": hex::encode(&payload[23..39]),
                    }),
                ),
                (
                    "packed20Bit",
                    json!({
                        "at59LowNibble": u16_le(payload, 59) as u32 | (((payload[67] & 0x0f) as u32) << 16),
                        "at61HighNibble": u16_le(payload, 61) as u32 | (((payload[67] >> 4) as u32) << 16),
                        "at63LowNibble": u16_le(payload, 63) as u32 | (((payload[68] & 0x0f) as u32) << 16),
                        "at65HighNibble": u16_le(payload, 65) as u32 | (((payload[68] >> 4) as u32) << 16),
                    }),
                ),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_CONNECTION_INTERFACE if payload.len() >= 2 => with_fields(
            base,
            &[
                ("kind", json!("connection-interface")),
                ("interface", json!(payload[1])),
            ],
        ),
        COMMAND_HANDSET_PROTOCOL => with_fields(
            base,
            &[
                ("kind", json!("handset-protocol-ack")),
                ("data", json!(hex::encode(&payload[1..]))),
            ],
        ),
        COMMAND_HANDSET if payload.len() >= 4 => {
            let expected_wrapped_length = payload[2];
            with_fields(
                base,
                &[
                    ("kind", json!("xcp-wrapper")),
                    ("subcommand", json!(payload[1])),
                    ("expectedWrappedLength", json!(expected_wrapped_length)),
                    ("marker", json!(payload[3])),
                    (
                        "markerDescription",
                        json!(if payload[3] == 0xff {
                            "xcp-response"
                        } else {
                            "non-xcp-or-error"
                        }),
                    ),
                    (
                        "wrappedLengthMatches",
                        json!(payload.len() == expected_wrapped_length as usize + 1),
                    ),
                    ("xcpData", json!(hex::encode(&payload[4..]))),
                ],
            )
        }
        _ => with_fields(base, &[("data", json!(hex::encode(&payload[1..])))]),
    }
}

fn with_fields(mut base: BTreeMap<String, Value>, fields: &[(&str, Value)]) -> Value {
    for (key, value) in fields {
        base.insert((*key).to_string(), value.clone());
    }
    json!(base)
}

fn result_description(result: u8) -> &'static str {
    match result {
        0x00 => "ok",
        0x01 => "unknown-command",
        0x02 => "unimplemented-command",
        0x03 => "command-not-allowed",
        0x04 => "invalid-command-length",
        0x05 => "user-not-logged-in",
        0x06 => "invalid-command-parameter",
        0x07 => "user-profile-not-ready-or-login-invalid",
        0x08 => "login-disabled",
        0x09 => "service-unavailable",
        0x0a => "firmware-hardware-incompatible",
        _ => "unknown-result",
    }
}

fn u16_le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn u64_le(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}
