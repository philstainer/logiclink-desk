use std::time::Duration;

use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification,
    WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::{StreamExt, stream::BoxStream};
use serde::Serialize;
use thiserror::Error;
use tokio::time::{Instant, sleep, timeout};
use uuid::Uuid;

use crate::commands::{CommandArg, command_payload, read_only_commands};
use crate::protocol::{
    GATT_SERVICE, decode_slip_stream, encode_packet, resolve_characteristic, slip_encode,
};

#[derive(Debug, Clone)]
pub struct QueryOptions {
    pub target_name: String,
    pub target_address: String,
    pub query: String,
    pub args: Vec<CommandArg>,
    pub characteristic: String,
    pub scan_timeout: Duration,
    pub connect_timeout: Duration,
    pub response_window: Duration,
    pub allow_motion: bool,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    #[serde(rename = "targetName")]
    pub target_name: String,
    pub query: String,
    pub characteristic: String,
    pub wrote: String,
    #[serde(rename = "requestedCommand")]
    pub requested_command: String,
    #[serde(rename = "matchedNotifications")]
    pub matched_notifications: Vec<serde_json::Value>,
    #[serde(rename = "otherNotifications")]
    pub other_notifications: Vec<serde_json::Value>,
    pub notifications: Vec<serde_json::Value>,
}

pub struct DeskSession {
    target_name: String,
    peripheral: Peripheral,
    characteristic: Characteristic,
    characteristic_uuid: Uuid,
    notifications: BoxStream<'static, ValueNotification>,
}

#[derive(Debug, Error)]
pub enum BleError {
    #[error("query command is not read-only: {0}")]
    QueryNotReadOnly(String),
    #[error("refusing get-interface-stats with resetFlag != 0")]
    InterfaceStatsResetRefused,
    #[error("no Bluetooth adapters found")]
    NoAdapters,
    #[error("did not discover {target_name} / {target_address} within {timeout_ms} ms")]
    NotDiscovered {
        target_name: String,
        target_address: String,
        timeout_ms: u128,
    },
    #[error("LOGIClink service not found: {0}")]
    ServiceNotFound(String),
    #[error("LOGIClink characteristic not found: {0}")]
    CharacteristicNotFound(String),
    #[error(transparent)]
    Btleplug(#[from] btleplug::Error),
    #[error(transparent)]
    Command(#[from] crate::commands::CommandError),
    #[error(transparent)]
    Protocol(#[from] crate::protocol::ProtocolError),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error("operation timed out: {0}")]
    Timeout(String),
}

pub async fn run_query(options: QueryOptions) -> Result<QueryResult, BleError> {
    validate_read_only_query(&options)?;

    let query = options.query.clone();
    let args = options.args.clone();
    let response_window = options.response_window;
    let mut session = DeskSession::connect(options).await?;
    let result = session.send_command(&query, &args, response_window).await;
    session.disconnect().await;
    result
}

impl DeskSession {
    pub async fn connect(options: QueryOptions) -> Result<Self, BleError> {
        validate_read_only_query(&options)?;

        let manager = Manager::new().await?;
        let adapters = manager.adapters().await?;
        let adapter = adapters.into_iter().next().ok_or(BleError::NoAdapters)?;
        let peripheral = find_peripheral(&adapter, &options).await?;

        timeout_named(options.connect_timeout, "connect", peripheral.connect()).await?;
        peripheral.discover_services().await?;

        let service_uuid = parse_uuid(GATT_SERVICE)?;
        let characteristic_uuid = parse_uuid(resolve_characteristic(&options.characteristic)?)?;
        if !peripheral
            .services()
            .iter()
            .any(|service| service.uuid == service_uuid)
        {
            return Err(BleError::ServiceNotFound(GATT_SERVICE.to_string()));
        }

        let characteristic = peripheral
            .characteristics()
            .into_iter()
            .find(|candidate| candidate.uuid == characteristic_uuid)
            .ok_or_else(|| BleError::CharacteristicNotFound(characteristic_uuid.to_string()))?;

        let notifications = peripheral.notifications().await?;
        peripheral.subscribe(&characteristic).await?;
        sleep(Duration::from_millis(500)).await;

        Ok(Self {
            target_name: options.target_name,
            peripheral,
            characteristic,
            characteristic_uuid,
            notifications,
        })
    }

    pub async fn send_command(
        &mut self,
        query: &str,
        args: &[CommandArg],
        response_window: Duration,
    ) -> Result<QueryResult, BleError> {
        self.send_command_with_quiet(query, args, response_window, Duration::from_millis(75))
            .await
    }

    pub async fn send_command_with_quiet(
        &mut self,
        query: &str,
        args: &[CommandArg],
        response_window: Duration,
        quiet_window: Duration,
    ) -> Result<QueryResult, BleError> {
        let (command, payload) = command_payload(query, args)?;
        let frame = slip_encode(&encode_packet(command, &payload, None));
        let requested_command = format!("0x{command:02x}");

        self.peripheral
            .write(&self.characteristic, &frame, WriteType::WithoutResponse)
            .await?;

        let notifications = self
            .collect_until_command_response(command, response_window, quiet_window)
            .await;

        let decoded = decode_notifications(notifications);
        let (matched_notifications, other_notifications): (Vec<_>, Vec<_>) =
            decoded.iter().cloned().partition(|item| {
                item.get("command").and_then(|value| value.as_str()) == Some(&requested_command)
            });

        Ok(QueryResult {
            target_name: self.target_name.clone(),
            query: query.to_string(),
            characteristic: self.characteristic_uuid.simple().to_string(),
            wrote: hex::encode(frame),
            requested_command,
            matched_notifications,
            other_notifications,
            notifications: decoded,
        })
    }

    pub async fn write_command(
        &mut self,
        query: &str,
        args: &[CommandArg],
    ) -> Result<String, BleError> {
        let (command, payload) = command_payload(query, args)?;
        let frame = slip_encode(&encode_packet(command, &payload, None));
        self.peripheral
            .write(&self.characteristic, &frame, WriteType::WithoutResponse)
            .await?;
        Ok(hex::encode(frame))
    }

    pub async fn drain_notifications(
        &mut self,
        response_window: Duration,
    ) -> Vec<serde_json::Value> {
        decode_notifications(self.collect_notifications(response_window).await)
    }

    pub async fn drain_notifications_timed(
        &mut self,
        response_window: Duration,
        started_at: std::time::Instant,
    ) -> Vec<serde_json::Value> {
        decode_timed_notifications(
            self.collect_timed_notifications(response_window, started_at)
                .await,
        )
    }

    pub async fn drain_available_notifications(&mut self) -> Vec<serde_json::Value> {
        let mut notifications = Vec::new();
        while let Ok(Some(notification)) =
            timeout(Duration::from_millis(1), self.notifications.next()).await
        {
            notifications.push(notification.value);
        }
        decode_notifications(notifications)
    }

    pub async fn drain_available_notifications_timed(
        &mut self,
        started_at: std::time::Instant,
    ) -> Vec<serde_json::Value> {
        let mut notifications = Vec::new();
        while let Ok(Some(notification)) =
            timeout(Duration::from_millis(1), self.notifications.next()).await
        {
            notifications.push((notification.value, started_at.elapsed().as_millis()));
        }
        decode_timed_notifications(notifications)
    }

    async fn collect_notifications(&mut self, response_window: Duration) -> Vec<Vec<u8>> {
        let deadline = Instant::now() + response_window;
        let mut notifications = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining, self.notifications.next()).await {
                Ok(Some(notification)) => notifications.push(notification.value),
                Ok(None) => break,
                Err(_) => break,
            }
        }
        notifications
    }

    async fn collect_timed_notifications(
        &mut self,
        response_window: Duration,
        started_at: std::time::Instant,
    ) -> Vec<(Vec<u8>, u128)> {
        let deadline = Instant::now() + response_window;
        let mut notifications = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining, self.notifications.next()).await {
                Ok(Some(notification)) => {
                    notifications.push((notification.value, started_at.elapsed().as_millis()));
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        notifications
    }

    async fn collect_until_command_response(
        &mut self,
        command: u8,
        response_window: Duration,
        quiet_window: Duration,
    ) -> Vec<Vec<u8>> {
        let deadline = Instant::now() + response_window;
        let mut notifications = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining, self.notifications.next()).await {
                Ok(Some(notification)) => {
                    let is_requested_command =
                        notification_matches_command(&notification.value, command);
                    notifications.push(notification.value);
                    if is_requested_command {
                        if !quiet_window.is_zero() {
                            notifications.extend(self.collect_quiet(quiet_window).await);
                        }
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        notifications
    }

    async fn collect_quiet(&mut self, quiet_window: Duration) -> Vec<Vec<u8>> {
        let mut notifications = Vec::new();
        while let Ok(Some(notification)) = timeout(quiet_window, self.notifications.next()).await {
            notifications.push(notification.value);
        }
        notifications
    }

    pub async fn disconnect(&self) {
        self.peripheral.unsubscribe(&self.characteristic).await.ok();
        self.peripheral.disconnect().await.ok();
    }
}

fn validate_read_only_query(options: &QueryOptions) -> Result<(), BleError> {
    let motion_commands = ["drive-up", "drive-down", "drive-to"];
    let is_read_only = read_only_commands().contains(&options.query.as_str());
    let is_allowed_motion =
        options.allow_motion && motion_commands.contains(&options.query.as_str());
    if !(is_read_only || is_allowed_motion) {
        return Err(BleError::QueryNotReadOnly(options.query.clone()));
    }
    if options.query == "get-interface-stats" {
        match options.args.get(1) {
            Some(CommandArg::Number(0)) | None => {}
            _ => return Err(BleError::InterfaceStatsResetRefused),
        }
    }
    Ok(())
}

async fn find_peripheral(
    adapter: &Adapter,
    options: &QueryOptions,
) -> Result<Peripheral, BleError> {
    adapter.start_scan(ScanFilter::default()).await?;
    let deadline = Instant::now() + options.scan_timeout;
    loop {
        for peripheral in adapter.peripherals().await? {
            if peripheral_matches(&peripheral, options).await? {
                adapter.stop_scan().await.ok();
                return Ok(peripheral);
            }
        }
        if Instant::now() >= deadline {
            adapter.stop_scan().await.ok();
            return Err(BleError::NotDiscovered {
                target_name: options.target_name.clone(),
                target_address: options.target_address.clone(),
                timeout_ms: options.scan_timeout.as_millis(),
            });
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn peripheral_matches(
    peripheral: &Peripheral,
    options: &QueryOptions,
) -> Result<bool, BleError> {
    let Some(properties) = peripheral.properties().await? else {
        return Ok(false);
    };
    let local_name = properties.local_name.unwrap_or_default();
    let address = properties.address.to_string().to_ascii_lowercase();
    Ok(local_name == options.target_name || address == options.target_address.to_ascii_lowercase())
}

fn decode_notifications(notifications: Vec<Vec<u8>>) -> Vec<serde_json::Value> {
    notifications
        .into_iter()
        .flat_map(|notification| decode_notification(&notification))
        .collect()
}

fn decode_timed_notifications(notifications: Vec<(Vec<u8>, u128)>) -> Vec<serde_json::Value> {
    notifications
        .into_iter()
        .flat_map(|(notification, received_at_ms)| {
            decode_notification(&notification)
                .into_iter()
                .map(move |mut value| {
                    if let Some(object) = value.as_object_mut() {
                        object.insert(
                            "receivedAtMs".to_string(),
                            serde_json::json!(received_at_ms),
                        );
                    }
                    value
                })
        })
        .collect()
}

fn decode_notification(notification: &[u8]) -> Vec<serde_json::Value> {
    let raw_notification = hex::encode(notification);
    match decode_slip_stream(notification) {
        Ok(decoded) => decoded
            .frames
            .into_iter()
            .map(move |frame| {
                serde_json::json!({
                    "rawNotification": raw_notification,
                    "rawPacket": hex::encode(frame.raw_packet),
                    "command": frame.command_hex,
                    "commandName": frame.command_name,
                    "status": frame.status,
                    "payload": hex::encode(frame.payload),
                    "parsed": frame.parsed,
                    "trailingRemainder": hex::encode(&decoded.remainder),
                })
            })
            .collect(),
        Err(error) => vec![serde_json::json!({
            "rawNotification": raw_notification,
            "error": error.to_string(),
        })],
    }
}

fn notification_matches_command(notification: &[u8], command: u8) -> bool {
    match decode_slip_stream(notification) {
        Ok(decoded) => decoded.frames.iter().any(|frame| frame.command == command),
        Err(_) => false,
    }
}

async fn timeout_named<T, F>(
    duration: Duration,
    name: &'static str,
    future: F,
) -> Result<T, BleError>
where
    F: std::future::Future<Output = Result<T, btleplug::Error>>,
{
    timeout(duration, future)
        .await
        .map_err(|_| BleError::Timeout(name.to_string()))?
        .map_err(BleError::from)
}

fn parse_uuid(raw: &str) -> Result<Uuid, uuid::Error> {
    Uuid::parse_str(raw)
}
