use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification,
    WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::{StreamExt, stream::BoxStream};
use rand::RngCore;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout};
use uuid::Uuid;

use crate::commands::{CommandArg, command_payload, read_only_commands};
use crate::protocol::{
    GATT_SERVICE, decode_slip_stream, encode_packet, resolve_characteristic, slip_encode,
};

const DEFAULT_DEVICE_NAME_PREFIX: &str = "LOGIClink";

#[derive(Debug, Clone)]
pub struct QueryOptions {
    pub device_name: Option<String>,
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
    #[serde(rename = "deviceName")]
    pub device_name: String,
    pub query: String,
    pub characteristic: String,
    pub wrote: String,
    #[serde(rename = "requestedCommand")]
    pub requested_command: String,
    #[serde(rename = "requestedNonce")]
    pub requested_nonce: String,
    #[serde(rename = "matchKind")]
    pub match_kind: String,
    #[serde(rename = "matchedNotifications")]
    pub matched_notifications: Vec<serde_json::Value>,
    #[serde(rename = "otherNotifications")]
    pub other_notifications: Vec<serde_json::Value>,
    pub notifications: Vec<serde_json::Value>,
}

pub struct DeskSession {
    device_name: String,
    peripheral: Peripheral,
    characteristic: Characteristic,
    characteristic_uuid: Uuid,
    notifications: Option<BoxStream<'static, ValueNotification>>,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeightSample {
    #[serde(rename = "receivedAtMs")]
    pub received_at_ms: u128,
    pub height: i64,
}

#[derive(Default)]
struct HeightMonitorState {
    latest_height: Option<i64>,
    samples: Vec<HeightSample>,
    polls: u32,
}

pub struct HeightMonitor {
    state: Arc<StdMutex<HeightMonitorState>>,
    stop: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

#[derive(Debug, Error)]
pub enum BleError {
    #[error("query command is not read-only: {0}")]
    QueryNotReadOnly(String),
    #[error("refusing get-interface-stats with resetFlag != 0")]
    InterfaceStatsResetRefused,
    #[error("no Bluetooth adapters found")]
    NoAdapters,
    #[error("did not discover {device_name} within {timeout_ms} ms")]
    NotDiscovered {
        device_name: String,
        timeout_ms: u128,
    },
    #[error("found multiple matching desks: {0}; pass --device-name to choose one")]
    MultipleDevices(String),
    #[error("discovered matching desk without a Bluetooth device name")]
    MissingDeviceName,
    #[error("height monitor is already running for this session")]
    HeightMonitorAlreadyStarted,
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
        let (peripheral, device_name) = find_peripheral(&adapter, &options).await?;

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
            device_name,
            peripheral,
            characteristic,
            characteristic_uuid,
            notifications: Some(notifications),
            write_lock: Arc::new(Mutex::new(())),
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
        let nonce = random_nonce();
        let frame = slip_encode(&encode_packet(command, &payload, Some(nonce)));
        let requested_command = format!("0x{command:02x}");
        let requested_nonce = hex::encode(nonce);

        self.write_frame(&frame).await?;

        let notifications = self
            .collect_until_command_response(command, nonce, response_window, quiet_window)
            .await;

        let decoded = decode_notifications(notifications);
        let exact_matches = decoded
            .iter()
            .filter(|item| {
                notification_value_matches_request(item, &requested_command, &requested_nonce)
            })
            .cloned()
            .collect::<Vec<_>>();
        let command_matches = decoded
            .iter()
            .filter(|item| notification_value_matches_command(item, &requested_command))
            .cloned()
            .collect::<Vec<_>>();
        let exact_match_found = !exact_matches.is_empty();
        let command_match_found = !command_matches.is_empty();
        let (matched_notifications, match_kind) = if exact_match_found {
            (exact_matches, "command-and-nonce")
        } else if command_match_found {
            (command_matches, "command-only-fallback")
        } else {
            (Vec::new(), "none")
        };
        let other_notifications = decoded
            .iter()
            .filter(|item| {
                if exact_match_found {
                    !notification_value_matches_request(item, &requested_command, &requested_nonce)
                } else {
                    !notification_value_matches_command(item, &requested_command)
                }
            })
            .cloned()
            .collect::<Vec<_>>();

        Ok(QueryResult {
            device_name: self.device_name.clone(),
            query: query.to_string(),
            characteristic: self.characteristic_uuid.simple().to_string(),
            wrote: hex::encode(frame),
            requested_command,
            requested_nonce,
            match_kind: match_kind.to_string(),
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
        self.write_frame(&frame).await?;
        Ok(hex::encode(frame))
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn start_height_monitor(
        &mut self,
        poll_interval: Duration,
        started_at: std::time::Instant,
    ) -> Result<HeightMonitor, BleError> {
        let mut notifications = self
            .notifications
            .take()
            .ok_or(BleError::HeightMonitorAlreadyStarted)?;
        let (command, payload) = command_payload("get-height", &[])?;
        let frame = slip_encode(&encode_packet(command, &payload, None));
        let peripheral = self.peripheral.clone();
        let characteristic = self.characteristic.clone();
        let write_lock = self.write_lock.clone();
        let state = Arc::new(StdMutex::new(HeightMonitorState::default()));
        let task_state = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let task_stop = stop.clone();
        let task = tokio::spawn(async move {
            let mut next_poll_at = tokio::time::Instant::now();
            while !task_stop.load(Ordering::Relaxed) {
                let now = tokio::time::Instant::now();
                if now >= next_poll_at {
                    let _guard = write_lock.lock().await;
                    if peripheral
                        .write(&characteristic, &frame, WriteType::WithoutResponse)
                        .await
                        .is_ok()
                        && let Ok(mut state) = task_state.lock()
                    {
                        state.polls += 1;
                    }
                    next_poll_at = now + poll_interval;
                }

                match timeout(Duration::from_millis(10), notifications.next()).await {
                    Ok(Some(notification)) => {
                        for value in decode_notification(&notification.value) {
                            if let Some(height) = notification_height(&value) {
                                let sample = HeightSample {
                                    received_at_ms: started_at.elapsed().as_millis(),
                                    height,
                                };
                                if let Ok(mut state) = task_state.lock() {
                                    state.latest_height = Some(height);
                                    state.samples.push(sample);
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
        });
        Ok(HeightMonitor { state, stop, task })
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
            timeout(Duration::from_millis(1), self.notifications_mut().next()).await
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
            timeout(Duration::from_millis(1), self.notifications_mut().next()).await
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
            match timeout(remaining, self.notifications_mut().next()).await {
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
            match timeout(remaining, self.notifications_mut().next()).await {
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
        nonce: [u8; 2],
        response_window: Duration,
        quiet_window: Duration,
    ) -> Vec<Vec<u8>> {
        let deadline = Instant::now() + response_window;
        let mut notifications = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining, self.notifications_mut().next()).await {
                Ok(Some(notification)) => {
                    let is_requested_command =
                        notification_matches_request(&notification.value, command, nonce);
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
        while let Ok(Some(notification)) =
            timeout(quiet_window, self.notifications_mut().next()).await
        {
            notifications.push(notification.value);
        }
        notifications
    }

    async fn write_frame(&self, frame: &[u8]) -> Result<(), BleError> {
        let _guard = self.write_lock.lock().await;
        self.peripheral
            .write(&self.characteristic, frame, WriteType::WithoutResponse)
            .await?;
        Ok(())
    }

    fn notifications_mut(&mut self) -> &mut BoxStream<'static, ValueNotification> {
        self.notifications
            .as_mut()
            .expect("notification stream is owned by height monitor")
    }

    pub async fn disconnect(&self) {
        self.peripheral.unsubscribe(&self.characteristic).await.ok();
        self.peripheral.disconnect().await.ok();
    }
}

impl HeightMonitor {
    pub fn latest_height(&self) -> Option<i64> {
        self.state.lock().ok()?.latest_height
    }

    pub fn samples_since(&self, index: usize) -> (usize, Vec<HeightSample>) {
        let Ok(state) = self.state.lock() else {
            return (index, Vec::new());
        };
        let next_index = state.samples.len();
        let samples = state.samples.get(index..).unwrap_or_default().to_vec();
        (next_index, samples)
    }

    pub fn poll_count(&self) -> u32 {
        self.state
            .lock()
            .map(|state| state.polls)
            .unwrap_or_default()
    }

    pub async fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.task.await.ok();
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
) -> Result<(Peripheral, String), BleError> {
    adapter.start_scan(ScanFilter::default()).await?;
    let deadline = Instant::now() + options.scan_timeout;
    loop {
        let mut matches = Vec::new();
        for peripheral in adapter.peripherals().await? {
            if let Some(device_name) = matching_device_name(&peripheral, options).await? {
                matches.push((peripheral, device_name));
            }
        }
        if options.device_name.is_some() {
            if let Some((peripheral, device_name)) = matches.into_iter().next() {
                adapter.stop_scan().await.ok();
                return Ok((peripheral, device_name));
            }
        } else if matches.len() == 1 {
            let (peripheral, device_name) = matches.remove(0);
            adapter.stop_scan().await.ok();
            return Ok((peripheral, device_name));
        } else if matches.len() > 1 {
            adapter.stop_scan().await.ok();
            let names = matches
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(BleError::MultipleDevices(names));
        }
        if Instant::now() >= deadline {
            adapter.stop_scan().await.ok();
            return Err(BleError::NotDiscovered {
                device_name: options
                    .device_name
                    .clone()
                    .unwrap_or_else(|| format!("{DEFAULT_DEVICE_NAME_PREFIX} device")),
                timeout_ms: options.scan_timeout.as_millis(),
            });
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn matching_device_name(
    peripheral: &Peripheral,
    options: &QueryOptions,
) -> Result<Option<String>, BleError> {
    let Some(properties) = peripheral.properties().await? else {
        return Ok(None);
    };
    let local_name = properties.local_name.unwrap_or_default();
    if options
        .device_name
        .as_deref()
        .is_some_and(|device_name| local_name == device_name)
        || options.device_name.is_none() && local_name.contains(DEFAULT_DEVICE_NAME_PREFIX)
    {
        if local_name.is_empty() {
            return Err(BleError::MissingDeviceName);
        }
        return Ok(Some(local_name));
    }
    Ok(None)
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
                    "nonce": frame.nonce,
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

fn notification_matches_request(notification: &[u8], command: u8, nonce: [u8; 2]) -> bool {
    let nonce = hex::encode(nonce);
    match decode_slip_stream(notification) {
        Ok(decoded) => decoded
            .frames
            .iter()
            .any(|frame| frame.command == command && frame.nonce == nonce),
        Err(_) => false,
    }
}

fn notification_value_matches_request(
    value: &serde_json::Value,
    command: &str,
    nonce: &str,
) -> bool {
    notification_value_matches_command(value, command)
        && value.get("nonce").and_then(|value| value.as_str()) == Some(nonce)
}

fn notification_value_matches_command(value: &serde_json::Value, command: &str) -> bool {
    value.get("command").and_then(|value| value.as_str()) == Some(command)
}

fn notification_height(value: &serde_json::Value) -> Option<i64> {
    value.get("parsed")?.get("height")?.as_i64()
}

fn random_nonce() -> [u8; 2] {
    let mut nonce = [0_u8; 2];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
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
