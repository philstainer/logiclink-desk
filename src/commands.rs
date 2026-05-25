use thiserror::Error;

pub const COMMAND_HEIGHT: u8 = 0x08;
pub const COMMAND_OCCUPANCY: u8 = 0x09;
pub const COMMAND_LED: u8 = 0x0a;
pub const COMMAND_BLE_GADGET: u8 = 0x11;
pub const COMMAND_HANDSET: u8 = 0x19;
pub const COMMAND_BUTTONS_BIND: u8 = 0x1a;
pub const COMMAND_LED_BIND: u8 = 0x1b;
pub const COMMAND_SERIAL_NUMBER: u8 = 0x1c;
pub const COMMAND_FIRMWARE_ID: u8 = 0x1d;
pub const COMMAND_TABLE_STATS: u8 = 0x21;
pub const COMMAND_INTERFACE_STATS: u8 = 0x22;
pub const COMMAND_PORT_INPUT: u8 = 0x27;
pub const COMMAND_CONNECTION_INTERFACE: u8 = 0x2b;
pub const COMMAND_HANDSET_PROTOCOL: u8 = 0x30;

#[derive(Debug, Clone)]
pub enum CommandArg {
    Number(u64),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("{0} requires an integer argument")]
    MissingNumber(String),
    #[error("expected u8 value, got {0}")]
    InvalidU8(u64),
    #[error("expected u16 value, got {0}")]
    InvalidU16(u64),
    #[error("value is longer than {max} bytes")]
    StringTooLong { max: usize },
}

pub fn command_payload(name: &str, args: &[CommandArg]) -> Result<(u8, Vec<u8>), CommandError> {
    match name {
        "get-height" => Ok((COMMAND_HEIGHT, vec![0x00, 0x00, 0x00])),
        "drive-up" => Ok((
            COMMAND_HEIGHT,
            concat_bytes(&[&[0x01], &u16le(number(args, 0).unwrap_or(1))?]),
        )),
        "drive-down" => Ok((
            COMMAND_HEIGHT,
            concat_bytes(&[&[0xff], &u16le(number(args, 0).unwrap_or(1))?]),
        )),
        "drive-to" => Ok((
            COMMAND_HEIGHT,
            concat_bytes(&[
                &[0x00, 0x00, 0x00],
                &u16le(
                    number(args, 0)
                        .ok_or_else(|| CommandError::MissingNumber("drive-to".to_string()))?,
                )?,
            ]),
        )),
        "get-occupancy" => Ok((COMMAND_OCCUPANCY, vec![0x00])),
        "led-on" => Ok((
            COMMAND_LED,
            led_payload(
                number(args, 0).unwrap_or(0),
                number(args, 1).unwrap_or(0),
                true,
            )?,
        )),
        "led-off" => Ok((
            COMMAND_LED,
            led_payload(
                number(args, 0).unwrap_or(0),
                number(args, 1).unwrap_or(0),
                false,
            )?,
        )),
        "led-bind" => Ok((
            COMMAND_LED_BIND,
            vec![u8_value(number(args, 0).unwrap_or(1))?],
        )),
        "buttons-bind" => Ok((
            COMMAND_BUTTONS_BIND,
            vec![u8_value(number(args, 0).unwrap_or(1))?],
        )),
        "all-buttons-bind" => {
            let bind = u8_value(number(args, 0).unwrap_or(1))?;
            Ok((COMMAND_BUTTONS_BIND, vec![bind, bind]))
        }
        "get-serial-number" => Ok((COMMAND_SERIAL_NUMBER, vec![0x00])),
        "get-fw-id" => Ok((
            COMMAND_FIRMWARE_ID,
            vec![u8_value(number(args, 0).unwrap_or(0))?],
        )),
        "get-table-stats" => Ok((COMMAND_TABLE_STATS, vec![0x00])),
        "get-interface-stats" => Ok((
            COMMAND_INTERFACE_STATS,
            vec![
                u8_value(number(args, 0).unwrap_or(0))?,
                u8_value(number(args, 1).unwrap_or(0))? & 0x01,
            ],
        )),
        "get-port-input" => Ok((COMMAND_PORT_INPUT, vec![0x00])),
        "get-conn-interface" => Ok((COMMAND_CONNECTION_INTERFACE, vec![0x00])),
        "handset-protocol" => Ok((
            COMMAND_HANDSET_PROTOCOL,
            vec![u8_value(number(args, 0).unwrap_or(0))?],
        )),
        "handset-command" => handset_command(args),
        "xcp-command" => xcp_command(args),
        "xcp-group-heights" => Ok((
            COMMAND_HANDSET,
            xcp_wrapper(&[0xf4, 0x0a, 0x00, 0x02, 0x00, 0x45, 0x00, 0x00], 10, 0)?,
        )),
        "xcp-init-group-count" => Ok((
            COMMAND_HANDSET,
            xcp_wrapper(&[0xf4, 0x01, 0x00, 0x02, 0x00, 0x6a, 0x00, 0x00], 1, 0)?,
        )),
        "xcp-init-channel-map" => Ok((
            COMMAND_HANDSET,
            xcp_wrapper(&[0xf4, 0x0a, 0x00, 0x02, 0x00, 0x6d, 0x00, 0x00], 10, 0)?,
        )),
        "xcp-init-calibration-records" => {
            let group_count = u8_value(number(args, 0).unwrap_or(2))?;
            let response_bytes = group_count.saturating_mul(8);
            Ok((
                COMMAND_HANDSET,
                xcp_wrapper(
                    &[0xf4, response_bytes, 0x00, 0x02, 0x69, 0x00, 0x00, 0x00],
                    response_bytes.into(),
                    0,
                )?,
            ))
        }
        "xcp-drive-to-group-raw" => Ok((
            COMMAND_HANDSET,
            concat_bytes(&[
                &[0x02, 0x00, 0x03, 0x08],
                &xcp_drive_to_group_raw(
                    number(args, 0).unwrap_or(0),
                    number(args, 1).unwrap_or(0),
                )?,
            ]),
        )),
        "ble-set-name" => Ok((
            COMMAND_BLE_GADGET,
            concat_bytes(&[&[0x01, 0x07, 0x0e], &ascii_arg(args, 0, 16)?]),
        )),
        "ble-set-key" => Ok((
            COMMAND_BLE_GADGET,
            concat_bytes(&[&[0x01, 0x09, 0x0e], &ascii_arg(args, 0, 6)?]),
        )),
        "ble-gadget-read" => Ok((
            COMMAND_BLE_GADGET,
            concat_bytes(&[
                &[0x02, 0x01, 0x0e],
                &u16le(number(args, 0).unwrap_or(0))?,
                &u16le(number(args, 1).unwrap_or(0))?,
            ]),
        )),
        "ble-gadget-write" => {
            let data = bytes(args, 2).unwrap_or_default();
            Ok((
                COMMAND_BLE_GADGET,
                concat_bytes(&[
                    &[0x02, 0x00, 0x0e],
                    &u16le(number(args, 0).unwrap_or(0))?,
                    &u16le(number(args, 1).unwrap_or(0))?,
                    &[u8_value(data.len() as u64)?],
                    &data,
                ]),
            ))
        }
        _ => Err(CommandError::UnknownCommand(name.to_string())),
    }
}

pub fn read_only_commands() -> &'static [&'static str] {
    &[
        "get-height",
        "get-occupancy",
        "get-serial-number",
        "get-fw-id",
        "get-table-stats",
        "get-interface-stats",
        "get-port-input",
        "get-conn-interface",
        "xcp-init-group-count",
        "xcp-init-channel-map",
        "xcp-init-calibration-records",
        "xcp-group-heights",
    ]
}

fn handset_command(args: &[CommandArg]) -> Result<(u8, Vec<u8>), CommandError> {
    let subcommand = u8_value(number(args, 0).unwrap_or(0))?;
    let relaxed = u8_value(number(args, 1).unwrap_or(0))? & 0x01;
    let packet_type = u8_value(number(args, 2).unwrap_or(0))?;
    let data = bytes(args, 3).unwrap_or_default();
    let payload = if data.is_empty() {
        vec![subcommand, relaxed]
    } else {
        concat_bytes(&[
            &[
                subcommand,
                relaxed,
                packet_type,
                u8_value(data.len() as u64)?,
            ],
            &data,
        ])
    };
    Ok((COMMAND_HANDSET, payload))
}

fn xcp_command(args: &[CommandArg]) -> Result<(u8, Vec<u8>), CommandError> {
    let relaxed = number(args, 0).unwrap_or(0);
    let data = bytes(args, 1).unwrap_or_default();
    let response_length = number(args, 2).unwrap_or(0);
    Ok((
        COMMAND_HANDSET,
        xcp_wrapper(&data, response_length, relaxed)?,
    ))
}

fn xcp_wrapper(request: &[u8], response_bytes: u64, relaxed: u64) -> Result<Vec<u8>, CommandError> {
    Ok(concat_bytes(&[
        &[
            0x02,
            u8_value(relaxed)? & 0x01,
            u8_value(response_bytes + 3)?,
            u8_value(request.len() as u64)?,
        ],
        request,
    ]))
}

fn xcp_drive_to_group_raw(group: u64, pulses: u64) -> Result<Vec<u8>, CommandError> {
    let group = u8_value(group)?;
    let pulses = u16le(pulses)?;
    let pulse_low = pulses[0];
    let pulse_high = pulses[1];
    let checksum = group ^ pulse_high ^ pulse_low ^ 0xc9;
    Ok(vec![
        0xc7, 0x96, 0x5f, group, 0x00, pulse_high, pulse_low, checksum,
    ])
}

fn led_payload(mask: u64, mode: u64, on: bool) -> Result<Vec<u8>, CommandError> {
    let selected = u8_value(mask)?;
    let mode_enabled = u8_value(mode)? != 0;
    let mut payload = vec![0x7f, 0x7f];
    if selected & 0x02 != 0 {
        payload[0] = if on {
            if mode_enabled { 0xfe } else { 0xff }
        } else if mode_enabled {
            0x01
        } else {
            0x00
        };
    }
    if selected & 0x01 != 0 {
        payload[1] = if on {
            if mode_enabled { 0xfe } else { 0xff }
        } else if mode_enabled {
            0x01
        } else {
            0x00
        };
    }
    Ok(payload)
}

fn ascii_arg(args: &[CommandArg], index: usize, max: usize) -> Result<Vec<u8>, CommandError> {
    let text = match args.get(index) {
        Some(CommandArg::Text(value)) => value.as_str(),
        Some(CommandArg::Number(_)) | Some(CommandArg::Bytes(_)) | None => "",
    };
    let bytes = text.as_bytes().to_vec();
    if bytes.len() > max {
        return Err(CommandError::StringTooLong { max });
    }
    Ok(bytes)
}

fn number(args: &[CommandArg], index: usize) -> Option<u64> {
    match args.get(index) {
        Some(CommandArg::Number(value)) => Some(*value),
        _ => None,
    }
}

fn bytes(args: &[CommandArg], index: usize) -> Option<Vec<u8>> {
    match args.get(index) {
        Some(CommandArg::Bytes(value)) => Some(value.clone()),
        _ => None,
    }
}

fn u8_value(value: u64) -> Result<u8, CommandError> {
    u8::try_from(value).map_err(|_| CommandError::InvalidU8(value))
}

fn u16le(value: u64) -> Result<[u8; 2], CommandError> {
    let value = u16::try_from(value).map_err(|_| CommandError::InvalidU16(value))?;
    Ok(value.to_le_bytes())
}

fn concat_bytes(chunks: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend_from_slice(chunk);
    }
    out
}
