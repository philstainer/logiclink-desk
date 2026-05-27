use std::time::Duration;

use clap::{Parser, Subcommand};
use logiclink_desk::ble::{DeskSession, QueryOptions, run_query};
use logiclink_desk::commands::{CommandArg, command_payload};
use logiclink_desk::protocol::{
    decode_packet, decode_slip_stream, encode_packet, resolve_characteristic, slip_encode,
};
use serde_json::json;

const SET_HEIGHT_TOLERANCE: i64 = 10;
const SET_HEIGHT_INTERVAL_MS: u64 = 180;
const SET_HEIGHT_UNITS_PER_TICK: f64 = 6.4;
const SET_HEIGHT_FINE_UNITS_PER_TICK: f64 = 3.0;
const SET_HEIGHT_FINE_SETTLE_MS: u64 = 2_500;
const SET_HEIGHT_BURST_SETTLE_MS: u64 = 2_500;
const SET_HEIGHT_RESPONSE_WINDOW_MS: u64 = 150;

#[derive(Debug, Parser)]
#[command(version, about = "LOGIClink desk control tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Build a command frame without using Bluetooth.
    Build {
        name: String,
        args: Vec<String>,
        /// Use a zero nonce for repeatable packet bytes.
        #[arg(long)]
        deterministic: bool,
    },
    /// Decode a SLIP stream or raw packet hex string.
    Decode {
        hex: String,
        #[arg(long)]
        packet: bool,
    },
    /// Run a guarded read-only BLE query.
    Query {
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "get-height")]
        query: String,
        #[arg(default_value = "app")]
        characteristic: String,
        args: Vec<String>,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 2_500)]
        response_window_ms: u64,
    },
    /// Read the current desk height.
    Height {
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 2_500)]
        response_window_ms: u64,
    },
    /// Send repeated drive-up or drive-down ticks and poll height after each tick.
    Pulse {
        #[arg(default_value = "up")]
        direction: String,
        /// Number of ticks to send, or maximum ticks when --target-height is set.
        #[arg(default_value_t = 5)]
        ticks: u16,
        /// Stop once the explicit get-height poll reaches or crosses this height.
        #[arg(long)]
        target_height: Option<i64>,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 750)]
        interval_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 2_500)]
        response_window_ms: u64,
    },
    /// Send repeated motion ticks without per-tick height polling, then read final height.
    Burst {
        #[arg(default_value = "up")]
        direction: String,
        #[arg(default_value_t = 20)]
        ticks: u16,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 5)]
        interval_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 400)]
        response_window_ms: u64,
    },
    /// Move toward a target height in centimetres using tuned live feedback.
    SetHeight {
        /// Target height in centimetres, e.g. 62 or 100.
        target_height_cm: f64,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
    },
    /// Move by a signed height delta in centimetres.
    AdjustHeight {
        /// Signed height delta in centimetres, e.g. 5 or -2.5.
        #[arg(allow_hyphen_values = true)]
        delta_cm: f64,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
    },
    /// Raise the desk by a height delta in centimetres.
    Raise {
        /// Height delta in centimetres, e.g. 5 or 2.5.
        delta_cm: f64,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
    },
    /// Lower the desk by a height delta in centimetres.
    Lower {
        /// Height delta in centimetres, e.g. 5 or 2.5.
        delta_cm: f64,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
    },
    /// Stream jog ticks and print live height notifications with derived speed.
    WatchMotion {
        #[arg(default_value = "up")]
        direction: String,
        #[arg(default_value_t = 20)]
        ticks: u16,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 100)]
        interval_ms: u64,
        #[arg(long, default_value_t = 1500)]
        drain_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 400)]
        response_window_ms: u64,
    },
    /// Profile motion tick response, notification latency, and coast after the last tick.
    ProfileMotion {
        #[arg(default_value = "up")]
        direction: String,
        #[arg(default_value_t = 10)]
        ticks: u16,
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 180)]
        interval_ms: u64,
        #[arg(long, default_value_t = 2_500)]
        drain_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 400)]
        response_window_ms: u64,
    },
    /// Benchmark connected get-height response latency at one or more requested intervals.
    BenchHeightPoll {
        #[arg(long)]
        device_name: Option<String>,
        #[arg(default_value = "app")]
        characteristic: String,
        /// Comma-separated requested intervals to test, in milliseconds.
        #[arg(long, default_value = "50,40,30,25,20,15,10")]
        intervals_ms: String,
        #[arg(long, default_value_t = 20)]
        samples: u16,
        /// Max time to wait for each get-height response.
        #[arg(long, default_value_t = 150)]
        response_wait_ms: u64,
        /// Extra quiet period after a matched response. Use 0 to return immediately.
        #[arg(long, default_value_t = 0)]
        response_quiet_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Build {
            name,
            args,
            deterministic,
        } => {
            let args = parse_command_args(&name, &args)?;
            let (command, payload) = command_payload(&name, &args)?;
            let packet = encode_packet(command, &payload, deterministic.then_some([0x00, 0x00]));
            let slip = slip_encode(&packet);
            let decoded = decode_packet(&packet)?;
            print_json(json!({
                "command": format!("0x{command:02x}"),
                "payload": hex::encode(&payload),
                "packet": hex::encode(&packet),
                "slip": hex::encode(&slip),
                "note": if deterministic {
                    "deterministic zero nonce used for repeatable output"
                } else {
                    "nonce is randomized; packet/slip bytes should differ per invocation"
                },
                "decodedCheck": {
                    "command": format!("0x{:02x}", decoded.command),
                    "payload": hex::encode(decoded.payload),
                },
            }))?;
        }
        Command::Decode { hex, packet } => {
            let bytes = parse_hex(&hex)?;
            if packet {
                let decoded = decode_packet(&bytes)?;
                print_json(json!({
                    "mode": "packet",
                    "frames": [{
                        "command": format!("0x{:02x}", decoded.command),
                        "status": decoded.status,
                        "payload": hex::encode(decoded.payload),
                        "plain": hex::encode(decoded.plain),
                    }],
                }))?;
            } else {
                print_json(json!({
                    "mode": "slip",
                    "frames": decode_slip_stream(&bytes)?.frames,
                }))?;
            }
        }
        Command::Query {
            device_name,
            query,
            characteristic,
            args,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
        } => {
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let args = parse_command_args(&query, &args)?;
            let result = run_query(QueryOptions {
                device_name,
                query,
                args,
                characteristic,
                scan_timeout: Duration::from_millis(scan_timeout_ms),
                connect_timeout: Duration::from_millis(connect_timeout_ms),
                response_window: Duration::from_millis(response_window_ms),
                allow_motion: false,
            })
            .await?;
            print_json(result)?;
        }
        Command::Height {
            device_name,
            characteristic,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
        } => {
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let response_window = Duration::from_millis(response_window_ms);
            let mut session = DeskSession::connect(query_options(
                device_name.as_deref(),
                "get-height",
                Vec::new(),
                &characteristic,
                scan_timeout_ms,
                connect_timeout_ms,
                response_window_ms,
                false,
            ))
            .await?;
            let height = read_height(&mut session, response_window).await?;
            session.disconnect().await;
            print_json(json!({
                "deviceName": session.device_name(),
                "action": "height",
                "height": height,
                "heightCm": height_units_to_cm(height),
            }))?;
        }
        Command::Pulse {
            direction,
            ticks,
            target_height,
            device_name,
            characteristic,
            interval_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
        } => {
            let motion_name = motion_command_name(&direction)?;
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let mut samples = Vec::new();
            let response_window = Duration::from_millis(response_window_ms);

            let mut session = DeskSession::connect(query_options(
                device_name.as_deref(),
                "get-height",
                Vec::new(),
                &characteristic,
                scan_timeout_ms,
                connect_timeout_ms,
                response_window_ms,
                false,
            ))
            .await?;
            let before = session
                .send_command("get-height", &[], response_window)
                .await?;
            let starting_height = parsed_height(&before);
            let mut final_height = starting_height;
            let mut stopped_reason = if target_reached(motion_name, starting_height, target_height)
            {
                Some("target-already-reached")
            } else {
                None
            };
            samples.push(json!({
                "label": "before",
                "height": starting_height,
                "result": before,
            }));

            for tick in 1..=ticks {
                if stopped_reason.is_some() {
                    break;
                }
                let motion_args = [CommandArg::Number(1)];
                let motion = session
                    .send_command(motion_name, &motion_args, response_window)
                    .await?;
                tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                let height = session
                    .send_command("get-height", &[], response_window)
                    .await?;
                let current_height = parsed_height(&height);
                final_height = current_height;
                if target_reached(motion_name, current_height, target_height) {
                    stopped_reason = Some("target-reached");
                }
                samples.push(json!({
                    "label": format!("tick-{tick}"),
                    "motionHeight": parsed_height(&motion),
                    "height": current_height,
                    "motion": motion,
                    "result": height,
                }));
            }
            session.disconnect().await;

            let stopped_reason = stopped_reason.unwrap_or("max-ticks-reached");
            print_json(json!({
                "deviceName": session.device_name(),
                "action": "pulse",
                "direction": motion_name,
                "ticks": ticks,
                "targetHeight": target_height,
                "stoppedReason": stopped_reason,
                "targetReached": stopped_reason == "target-already-reached" || stopped_reason == "target-reached",
                "intervalMs": interval_ms,
                "startingHeight": starting_height,
                "finalHeight": final_height,
                "observedDelta": match (starting_height, final_height) {
                    (Some(start), Some(end)) => Some(end - start),
                    _ => None,
                },
                "samples": samples,
            }))?;
        }
        Command::Burst {
            direction,
            ticks,
            device_name,
            characteristic,
            interval_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
        } => {
            let motion_name = motion_command_name(&direction)?;
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let response_window = Duration::from_millis(response_window_ms);
            let mut session = DeskSession::connect(query_options(
                device_name.as_deref(),
                "get-height",
                Vec::new(),
                &characteristic,
                scan_timeout_ms,
                connect_timeout_ms,
                response_window_ms,
                true,
            ))
            .await?;

            let before = session
                .send_command("get-height", &[], response_window)
                .await?;
            let starting_height = parsed_height(&before);
            let motion_args = [CommandArg::Number(1)];
            let started_at = std::time::Instant::now();
            let mut motions = Vec::new();

            for tick in 1..=ticks {
                let wrote = session.write_command(motion_name, &motion_args).await?;
                motions.push(json!({
                    "tick": tick,
                    "wrote": wrote,
                }));
                if tick < ticks && interval_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                }
            }

            let motion_notifications = session.drain_notifications(response_window).await;

            let after = session
                .send_command("get-height", &[], response_window)
                .await?;
            let final_height = parsed_height(&after);
            session.disconnect().await;

            print_json(json!({
                "deviceName": session.device_name(),
                "action": "burst",
                "direction": motion_name,
                "ticks": ticks,
                "intervalMs": interval_ms,
                "responseWindowMs": response_window_ms,
                "elapsedMs": started_at.elapsed().as_millis(),
                "startingHeight": starting_height,
                "finalHeight": final_height,
                "observedDelta": match (starting_height, final_height) {
                    (Some(start), Some(end)) => Some(end - start),
                    _ => None,
                },
                "before": before,
                "motions": motions,
                "motionNotifications": motion_notifications,
                "after": after,
            }))?;
        }
        Command::SetHeight {
            target_height_cm,
            device_name,
            characteristic,
            timeout_ms,
            scan_timeout_ms,
            connect_timeout_ms,
        } => {
            run_height_move(
                "set-height",
                HeightTarget::AbsoluteCm(target_height_cm),
                device_name,
                characteristic,
                timeout_ms,
                scan_timeout_ms,
                connect_timeout_ms,
            )
            .await?;
        }
        Command::AdjustHeight {
            delta_cm,
            device_name,
            characteristic,
            timeout_ms,
            scan_timeout_ms,
            connect_timeout_ms,
        } => {
            run_height_move(
                "adjust-height",
                HeightTarget::RelativeCm(delta_cm),
                device_name,
                characteristic,
                timeout_ms,
                scan_timeout_ms,
                connect_timeout_ms,
            )
            .await?;
        }
        Command::Raise {
            delta_cm,
            device_name,
            characteristic,
            timeout_ms,
            scan_timeout_ms,
            connect_timeout_ms,
        } => {
            run_height_move(
                "raise",
                HeightTarget::RelativeCm(positive_delta_cm(delta_cm)?),
                device_name,
                characteristic,
                timeout_ms,
                scan_timeout_ms,
                connect_timeout_ms,
            )
            .await?;
        }
        Command::Lower {
            delta_cm,
            device_name,
            characteristic,
            timeout_ms,
            scan_timeout_ms,
            connect_timeout_ms,
        } => {
            run_height_move(
                "lower",
                HeightTarget::RelativeCm(-positive_delta_cm(delta_cm)?),
                device_name,
                characteristic,
                timeout_ms,
                scan_timeout_ms,
                connect_timeout_ms,
            )
            .await?;
        }
        Command::WatchMotion {
            direction,
            ticks,
            device_name,
            characteristic,
            interval_ms,
            drain_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
        } => {
            let motion_name = motion_command_name(&direction)?;
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let response_window = Duration::from_millis(response_window_ms);
            let mut session = DeskSession::connect(query_options(
                device_name.as_deref(),
                "get-height",
                Vec::new(),
                &characteristic,
                scan_timeout_ms,
                connect_timeout_ms,
                response_window_ms,
                true,
            ))
            .await?;
            let before = session
                .send_command("get-height", &[], response_window)
                .await?;
            let before_height = parsed_height(&before);
            session.drain_notifications(Duration::from_millis(50)).await;

            let started_at = std::time::Instant::now();
            let mut events = Vec::new();
            let mut writes = Vec::new();
            let args = [CommandArg::Number(1)];
            let mut last_height: Option<(i64, u128)> = None;

            for tick in 1..=ticks {
                let tick_started_at = std::time::Instant::now();
                session.write_command(motion_name, &args).await?;
                writes.push(json!({
                    "tick": tick,
                    "atMs": started_at.elapsed().as_millis(),
                }));
                let notifications = session
                    .drain_available_notifications_timed(started_at)
                    .await;
                collect_motion_events(
                    &mut events,
                    notifications,
                    started_at,
                    &mut last_height,
                    Some(tick),
                );
                let cadence = Duration::from_millis(interval_ms);
                let elapsed = tick_started_at.elapsed();
                if elapsed < cadence {
                    tokio::time::sleep(cadence - elapsed).await;
                }
            }

            let notifications = session
                .drain_notifications_timed(Duration::from_millis(drain_ms), started_at)
                .await;
            collect_motion_events(
                &mut events,
                notifications,
                started_at,
                &mut last_height,
                None,
            );
            let after = session
                .send_command("get-height", &[], response_window)
                .await?;
            let after_height = parsed_height(&after);
            let analysis = motion_analysis(before_height, after_height, &writes, &events);
            session.disconnect().await;

            print_json(json!({
                "deviceName": session.device_name(),
                "action": "watch-motion",
                "direction": motion_name,
                "ticks": ticks,
                "intervalMs": interval_ms,
                "drainMs": drain_ms,
                "beforeHeight": before_height,
                "afterHeight": after_height,
                "observedDelta": match (before_height, after_height) {
                    (Some(start), Some(end)) => Some(end - start),
                    _ => None,
                },
                "writes": writes,
                "events": events,
                "analysis": analysis,
            }))?;
        }
        Command::BenchHeightPoll {
            device_name,
            characteristic,
            intervals_ms,
            samples,
            response_wait_ms,
            response_quiet_ms,
            scan_timeout_ms,
            connect_timeout_ms,
        } => {
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let intervals = parse_csv_u64(&intervals_ms)?;
            let response_window = Duration::from_millis(response_wait_ms);
            let response_quiet = Duration::from_millis(response_quiet_ms);
            let mut session = DeskSession::connect(query_options(
                device_name.as_deref(),
                "get-height",
                Vec::new(),
                &characteristic,
                scan_timeout_ms,
                connect_timeout_ms,
                response_wait_ms,
                false,
            ))
            .await?;

            let connected_at = std::time::Instant::now();
            let mut runs = Vec::new();
            for interval_ms in intervals {
                let requested_interval = Duration::from_millis(interval_ms);
                session
                    .drain_notifications(Duration::from_millis(100))
                    .await;

                let run_started_at = std::time::Instant::now();
                let mut sample_results = Vec::new();
                let mut response_latencies = Vec::new();
                let mut period_ms = Vec::new();
                let mut previous_start_ms: Option<u128> = None;

                for sample in 1..=samples {
                    let sample_started_at = std::time::Instant::now();
                    let sample_start_ms = run_started_at.elapsed().as_millis();
                    if let Some(previous_start_ms) = previous_start_ms {
                        period_ms.push(sample_start_ms.saturating_sub(previous_start_ms));
                    }
                    previous_start_ms = Some(sample_start_ms);

                    let result = session
                        .send_command_with_quiet("get-height", &[], response_window, response_quiet)
                        .await?;
                    let response_latency_ms = sample_started_at.elapsed().as_millis();
                    let height = parsed_height(&result);
                    if height.is_some() {
                        response_latencies.push(response_latency_ms);
                    }
                    sample_results.push(json!({
                        "sample": sample,
                        "startedAtMs": sample_start_ms,
                        "responseLatencyMs": response_latency_ms,
                        "height": height,
                        "matched": !result.matched_notifications.is_empty(),
                        "notificationCount": result.notifications.len(),
                    }));

                    let elapsed = sample_started_at.elapsed();
                    if sample < samples && elapsed < requested_interval {
                        tokio::time::sleep(requested_interval - elapsed).await;
                    }
                }

                runs.push(json!({
                    "requestedIntervalMs": interval_ms,
                    "samples": samples,
                    "elapsedMs": run_started_at.elapsed().as_millis(),
                    "heightResponses": response_latencies.len(),
                    "responseLatencyMs": latency_summary(&response_latencies),
                    "observedPeriodMs": latency_summary(&period_ms),
                    "sampleResults": sample_results,
                }));
            }
            session.disconnect().await;

            print_json(json!({
                "deviceName": session.device_name(),
                "action": "bench-height-poll",
                "connectedElapsedMs": connected_at.elapsed().as_millis(),
                "responseWaitMs": response_wait_ms,
                "responseQuietMs": response_quiet_ms,
                "runs": runs,
            }))?;
        }
        Command::ProfileMotion {
            direction,
            ticks,
            device_name,
            characteristic,
            interval_ms,
            drain_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
        } => {
            let result = run_motion_profile(
                "profile-motion",
                &direction,
                ticks,
                device_name,
                characteristic,
                interval_ms,
                drain_ms,
                scan_timeout_ms,
                connect_timeout_ms,
                response_window_ms,
            )
            .await?;
            print_json(result)?;
        }
    }

    Ok(())
}

enum HeightTarget {
    AbsoluteCm(f64),
    RelativeCm(f64),
}

#[allow(clippy::too_many_arguments)]
async fn run_motion_profile(
    action: &str,
    direction: &str,
    ticks: u16,
    device_name: Option<String>,
    characteristic: String,
    interval_ms: u64,
    drain_ms: u64,
    scan_timeout_ms: u64,
    connect_timeout_ms: u64,
    response_window_ms: u64,
) -> anyhow::Result<serde_json::Value> {
    let motion_name = motion_command_name(direction)?;
    let characteristic = resolve_characteristic(&characteristic)?.to_string();
    let response_window = Duration::from_millis(response_window_ms);
    let mut session = DeskSession::connect(query_options(
        device_name.as_deref(),
        "get-height",
        Vec::new(),
        &characteristic,
        scan_timeout_ms,
        connect_timeout_ms,
        response_window_ms,
        true,
    ))
    .await?;
    let before = session
        .send_command("get-height", &[], response_window)
        .await?;
    let before_height = parsed_height(&before);
    session.drain_notifications(Duration::from_millis(50)).await;

    let started_at = std::time::Instant::now();
    let mut events = Vec::new();
    let mut writes = Vec::new();
    let args = [CommandArg::Number(1)];
    let mut last_height: Option<(i64, u128)> = None;

    for tick in 1..=ticks {
        let tick_started_at = std::time::Instant::now();
        session.write_command(motion_name, &args).await?;
        writes.push(json!({
            "tick": tick,
            "atMs": started_at.elapsed().as_millis(),
        }));
        let notifications = session
            .drain_available_notifications_timed(started_at)
            .await;
        collect_motion_events(
            &mut events,
            notifications,
            started_at,
            &mut last_height,
            Some(tick),
        );
        let cadence = Duration::from_millis(interval_ms);
        let elapsed = tick_started_at.elapsed();
        if elapsed < cadence {
            tokio::time::sleep(cadence - elapsed).await;
        }
    }

    let notifications = session
        .drain_notifications_timed(Duration::from_millis(drain_ms), started_at)
        .await;
    collect_motion_events(
        &mut events,
        notifications,
        started_at,
        &mut last_height,
        None,
    );
    let after = session
        .send_command("get-height", &[], response_window)
        .await?;
    let after_height = parsed_height(&after);
    let analysis = motion_analysis(before_height, after_height, &writes, &events);
    let profile = motion_profile_summary(before_height, after_height, &writes, &events);
    let device_name = session.device_name().to_string();
    session.disconnect().await;

    Ok(json!({
        "deviceName": device_name,
        "action": action,
        "direction": motion_name,
        "ticks": ticks,
        "intervalMs": interval_ms,
        "drainMs": drain_ms,
        "beforeHeight": before_height,
        "afterHeight": after_height,
        "observedDelta": match (before_height, after_height) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        },
        "writes": writes,
        "events": events,
        "analysis": analysis,
        "profile": profile,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn run_height_move(
    action: &str,
    height_target: HeightTarget,
    device_name: Option<String>,
    characteristic: String,
    timeout_ms: u64,
    scan_timeout_ms: u64,
    connect_timeout_ms: u64,
) -> anyhow::Result<()> {
    let tolerance = SET_HEIGHT_TOLERANCE;
    let interval_ms = SET_HEIGHT_INTERVAL_MS;
    let units_per_tick = SET_HEIGHT_UNITS_PER_TICK;
    let fine_units_per_tick = SET_HEIGHT_FINE_UNITS_PER_TICK;
    let fine_threshold = units_per_tick.ceil() as i64 + tolerance;
    let fine_settle_ms = SET_HEIGHT_FINE_SETTLE_MS;
    let burst_settle_ms = SET_HEIGHT_BURST_SETTLE_MS;
    let response_window_ms = SET_HEIGHT_RESPONSE_WINDOW_MS;
    let characteristic = resolve_characteristic(&characteristic)?.to_string();
    let response_window = Duration::from_millis(response_window_ms);
    let mut session = DeskSession::connect(query_options(
        device_name.as_deref(),
        "get-height",
        Vec::new(),
        &characteristic,
        scan_timeout_ms,
        connect_timeout_ms,
        response_window_ms,
        true,
    ))
    .await?;

    let started_at = std::time::Instant::now();
    let mut samples = Vec::new();
    let initial_drained = session.drain_available_notifications().await;
    let initial_drained_heights: Vec<_> = initial_drained
        .iter()
        .filter_map(notification_height)
        .collect();
    let mut current = read_height(&mut session, response_window).await?;
    let starting_height = current;
    let (target_height, requested_delta_cm) = match height_target {
        HeightTarget::AbsoluteCm(target_height_cm) => {
            (height_units_from_cm(target_height_cm)?, None)
        }
        HeightTarget::RelativeCm(delta_cm) => {
            let delta_height = height_delta_units_from_cm(delta_cm)?;
            let target_height = current + delta_height;
            if target_height <= 0 {
                anyhow::bail!("target height must be positive");
            }
            (target_height, Some(delta_cm))
        }
    };
    samples.push(json!({
        "label": "start",
        "height": current,
        "heightCm": height_units_to_cm(current),
        "drainedHeights": initial_drained_heights,
        "deltaToTarget": target_height - current,
    }));

    let mut correction_count = 0_u32;
    while (target_height - current).abs() > tolerance {
        if started_at.elapsed() >= Duration::from_millis(timeout_ms) {
            anyhow::bail!("timed out moving to target: current={current} target={target_height}");
        }

        let previous_height = current;
        let delta = target_height - current;
        let direction = if delta > 0 { "drive-up" } else { "drive-down" };
        let remaining = delta.abs();
        let fine_mode = remaining <= fine_threshold || correction_count > 0;
        let planned_ticks = if correction_count > 0 {
            1
        } else if fine_mode {
            ((remaining.saturating_sub(tolerance).max(1) as f64) / fine_units_per_tick)
                .ceil()
                .max(1.0) as u32
        } else {
            ((remaining as f64) / units_per_tick).round().max(1.0) as u32
        };
        let planned_ticks = planned_ticks.max(1);
        let wrote = write_motion_ticks(
            &mut session,
            direction,
            planned_ticks,
            Duration::from_millis(interval_ms),
        )
        .await?;
        let settle_ms = if fine_mode {
            fine_settle_ms
        } else {
            burst_settle_ms
        };
        tokio::time::sleep(Duration::from_millis(settle_ms)).await;
        let drained = session.drain_available_notifications().await;
        let drained_heights: Vec<_> = drained.iter().filter_map(notification_height).collect();
        current = read_height(&mut session, response_window).await?;
        let observed_delta = current - previous_height;
        let overshot = (target_height - current).signum()
            != (target_height - previous_height).signum()
            && (target_height - current).abs() > tolerance;
        if overshot {
            correction_count += 1;
        }

        samples.push(json!({
            "label": format!("step-{}", samples.len()),
            "direction": direction,
            "fineMode": fine_mode,
            "ticks": planned_ticks,
            "wrote": wrote,
            "drainedHeights": drained_heights,
            "settleMs": settle_ms,
            "height": current,
            "heightCm": height_units_to_cm(current),
            "deltaToTarget": target_height - current,
            "observedDelta": observed_delta,
            "observedDeltaCm": height_units_to_cm(observed_delta),
            "observedUnitsPerTick": observed_delta as f64 / planned_ticks as f64,
            "correctionCount": correction_count,
            "elapsedMs": started_at.elapsed().as_millis(),
        }));

        if correction_count >= 4 {
            anyhow::bail!(
                "stopped after repeated overshoot corrections: current={current} target={target_height}"
            );
        }
    }

    session.disconnect().await;
    print_json(json!({
        "deviceName": session.device_name(),
        "action": action,
        "targetHeight": target_height,
        "targetHeightCm": height_units_to_cm(target_height),
        "requestedDeltaCm": requested_delta_cm,
        "tolerance": tolerance,
        "startingHeight": starting_height,
        "startingHeightCm": height_units_to_cm(starting_height),
        "finalHeight": current,
        "finalHeightCm": height_units_to_cm(current),
        "observedDelta": current - starting_height,
        "observedDeltaCm": height_units_to_cm(current - starting_height),
        "withinTolerance": (target_height - current).abs() <= tolerance,
        "elapsedMs": started_at.elapsed().as_millis(),
        "intervalMs": interval_ms,
        "unitsPerTick": units_per_tick,
        "fineUnitsPerTick": fine_units_per_tick,
        "fineThreshold": fine_threshold,
        "fineSettleMs": fine_settle_ms,
        "burstSettleMs": burst_settle_ms,
        "samples": samples,
    }))?;
    Ok(())
}

fn motion_command_name(direction: &str) -> anyhow::Result<&'static str> {
    match direction {
        "up" | "drive-up" => Ok("drive-up"),
        "down" | "drive-down" => Ok("drive-down"),
        _ => anyhow::bail!("direction must be up or down"),
    }
}

fn target_reached(
    motion_name: &str,
    current_height: Option<i64>,
    target_height: Option<i64>,
) -> bool {
    let (Some(current_height), Some(target_height)) = (current_height, target_height) else {
        return false;
    };
    match motion_name {
        "drive-up" => current_height >= target_height,
        "drive-down" => current_height <= target_height,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn query_options(
    device_name: Option<&str>,
    query: &str,
    args: Vec<CommandArg>,
    characteristic: &str,
    scan_timeout_ms: u64,
    connect_timeout_ms: u64,
    response_window_ms: u64,
    allow_motion: bool,
) -> QueryOptions {
    QueryOptions {
        device_name: device_name.map(str::to_string),
        query: query.to_string(),
        args,
        characteristic: characteristic.to_string(),
        scan_timeout: Duration::from_millis(scan_timeout_ms),
        connect_timeout: Duration::from_millis(connect_timeout_ms),
        response_window: Duration::from_millis(response_window_ms),
        allow_motion,
    }
}

fn parsed_height(result: &impl serde::Serialize) -> Option<i64> {
    serde_json::to_value(result)
        .ok()?
        .get("matchedNotifications")?
        .get(0)?
        .get("parsed")?
        .get("height")?
        .as_i64()
}

fn notification_height(value: &serde_json::Value) -> Option<i64> {
    value.get("parsed")?.get("height")?.as_i64()
}

fn notification_counter(value: &serde_json::Value) -> Option<i64> {
    value.get("parsed")?.get("movementCounter")?.as_i64()
}

fn height_units_from_cm(height_cm: f64) -> anyhow::Result<i64> {
    if !height_cm.is_finite() || height_cm <= 0.0 {
        anyhow::bail!("target height must be a positive centimetre value");
    }
    Ok((height_cm * 10.0).round() as i64)
}

fn height_delta_units_from_cm(delta_cm: f64) -> anyhow::Result<i64> {
    if !delta_cm.is_finite() || delta_cm == 0.0 {
        anyhow::bail!("height delta must be a non-zero centimetre value");
    }
    let delta_units = (delta_cm * 10.0).round() as i64;
    if delta_units == 0 {
        anyhow::bail!("height delta must round to at least 0.1 centimetres");
    }
    Ok(delta_units)
}

fn positive_delta_cm(delta_cm: f64) -> anyhow::Result<f64> {
    if !delta_cm.is_finite() || delta_cm <= 0.0 {
        anyhow::bail!("height delta must be a positive centimetre value");
    }
    Ok(delta_cm)
}

fn height_units_to_cm(height_units: i64) -> f64 {
    height_units as f64 / 10.0
}

async fn write_motion_ticks(
    session: &mut DeskSession,
    direction: &str,
    ticks: u32,
    interval: Duration,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut writes = Vec::new();
    let started_at = std::time::Instant::now();
    for tick in 1..=ticks {
        let tick_started_at = std::time::Instant::now();
        let wrote = session
            .write_command(direction, &[CommandArg::Number(1)])
            .await?;
        writes.push(json!({
            "tick": tick,
            "atMs": started_at.elapsed().as_millis(),
            "wrote": wrote,
        }));
        let elapsed = tick_started_at.elapsed();
        if tick < ticks && elapsed < interval {
            tokio::time::sleep(interval - elapsed).await;
        }
    }
    Ok(writes)
}

fn collect_motion_events(
    events: &mut Vec<serde_json::Value>,
    notifications: Vec<serde_json::Value>,
    started_at: std::time::Instant,
    last_height: &mut Option<(i64, u128)>,
    tick: Option<u16>,
) {
    for notification in notifications {
        let Some(height) = notification_height(&notification) else {
            continue;
        };
        let at_ms = notification
            .get("receivedAtMs")
            .and_then(|value| value.as_u64())
            .map(u128::from)
            .unwrap_or_else(|| started_at.elapsed().as_millis());
        let derived_speed_units_per_second =
            last_height.and_then(|(previous_height, previous_ms)| {
                let delta_ms = at_ms.checked_sub(previous_ms)?;
                (delta_ms > 0).then(|| (height - previous_height) as f64 * 1000.0 / delta_ms as f64)
            });
        *last_height = Some((height, at_ms));
        events.push(json!({
            "atMs": at_ms,
            "tick": tick,
            "height": height,
            "movementCounter": notification_counter(&notification),
            "payload": notification.get("payload").cloned(),
            "derivedSpeedUnitsPerSecond": derived_speed_units_per_second,
        }));
    }
}

fn motion_analysis(
    before_height: Option<i64>,
    after_height: Option<i64>,
    writes: &[serde_json::Value],
    events: &[serde_json::Value],
) -> serde_json::Value {
    let first_write_ms = writes
        .first()
        .and_then(|item| item.get("atMs"))
        .and_then(|value| value.as_u64());
    let last_write_ms = writes
        .last()
        .and_then(|item| item.get("atMs"))
        .and_then(|value| value.as_u64());
    let first_change = before_height.and_then(|start| {
        events.iter().find_map(|event| {
            let height = event.get("height")?.as_i64()?;
            (height != start).then_some(event)
        })
    });
    let first_change_ms = first_change
        .and_then(|event| event.get("atMs"))
        .and_then(|value| value.as_u64());
    let first_change_tick = first_change
        .and_then(|event| event.get("tick"))
        .and_then(|value| value.as_u64());
    let last_event = events.last();
    let last_event_ms = last_event
        .and_then(|event| event.get("atMs"))
        .and_then(|value| value.as_u64());
    let last_event_height = last_event
        .and_then(|event| event.get("height"))
        .and_then(|value| value.as_i64());
    let max_abs_speed = events
        .iter()
        .filter_map(|event| event.get("derivedSpeedUnitsPerSecond")?.as_f64())
        .map(f64::abs)
        .fold(0.0_f64, f64::max);

    json!({
        "firstWriteMs": first_write_ms,
        "lastWriteMs": last_write_ms,
        "firstHeightChangeMs": first_change_ms,
        "firstHeightChangeTick": first_change_tick,
        "heightChangeLatencyMs": match (first_write_ms, first_change_ms) {
            (Some(start), Some(change)) => change.checked_sub(start),
            _ => None,
        },
        "lastNotificationMs": last_event_ms,
        "lastNotificationHeight": last_event_height,
        "finalHeightMinusLastNotification": match (after_height, last_event_height) {
            (Some(after), Some(last)) => Some(after - last),
            _ => None,
        },
        "observedDelta": match (before_height, after_height) {
            (Some(before), Some(after)) => Some(after - before),
            _ => None,
        },
        "maxAbsDerivedSpeedUnitsPerSecond": max_abs_speed,
    })
}

fn motion_profile_summary(
    before_height: Option<i64>,
    after_height: Option<i64>,
    writes: &[serde_json::Value],
    events: &[serde_json::Value],
) -> serde_json::Value {
    let first_write_ms = writes
        .first()
        .and_then(|item| item.get("atMs"))
        .and_then(|value| value.as_u64());
    let last_write_ms = writes
        .last()
        .and_then(|item| item.get("atMs"))
        .and_then(|value| value.as_u64());
    let first_change = before_height.and_then(|start| {
        events.iter().find_map(|event| {
            let height = event.get("height")?.as_i64()?;
            (height != start).then_some(event)
        })
    });
    let first_change_ms = first_change
        .and_then(|event| event.get("atMs"))
        .and_then(|value| value.as_u64());
    let last_change = events.windows(2).rev().find_map(|pair| {
        let previous = pair[0].get("height")?.as_i64()?;
        let current = pair[1].get("height")?.as_i64()?;
        (previous != current).then_some(&pair[1])
    });
    let last_change_ms = last_change
        .and_then(|event| event.get("atMs"))
        .and_then(|value| value.as_u64());
    let last_change_height = last_change
        .and_then(|event| event.get("height"))
        .and_then(|value| value.as_i64());
    let first_change_height = first_change
        .and_then(|event| event.get("height"))
        .and_then(|value| value.as_i64());
    let movement_after_last_write = match (last_write_ms, events.last()) {
        (Some(last_write_ms), Some(_)) => {
            let height_at_last_write = events
                .iter()
                .rev()
                .find(|event| {
                    event
                        .get("atMs")
                        .and_then(|value| value.as_u64())
                        .is_some_and(|at_ms| at_ms <= last_write_ms)
                })
                .and_then(|event| event.get("height"))
                .and_then(|value| value.as_i64())
                .or(before_height);
            match (height_at_last_write, after_height) {
                (Some(start), Some(end)) => Some(end - start),
                _ => None,
            }
        }
        _ => None,
    };

    json!({
        "firstChangeLatencyMs": match (first_write_ms, first_change_ms) {
            (Some(write), Some(change)) => change.checked_sub(write),
            _ => None,
        },
        "lastChangeAfterLastWriteMs": match (last_write_ms, last_change_ms) {
            (Some(write), Some(change)) => change.checked_sub(write),
            _ => None,
        },
        "firstChangeHeight": first_change_height,
        "lastChangeHeight": last_change_height,
        "movementAfterLastWrite": movement_after_last_write,
        "unitsPerTick": match (before_height, after_height) {
            (Some(before), Some(after)) if !writes.is_empty() => {
                Some((after - before) as f64 / writes.len() as f64)
            }
            _ => None,
        },
    })
}

async fn read_height(session: &mut DeskSession, response_window: Duration) -> anyhow::Result<i64> {
    let result = session
        .send_command_with_quiet("get-height", &[], response_window, Duration::ZERO)
        .await?;
    result
        .matched_notifications
        .iter()
        .rev()
        .find_map(notification_height)
        .ok_or_else(|| anyhow::anyhow!("could not parse height response"))
}

fn print_json(value: impl serde::Serialize) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn parse_command_args(command_name: &str, args: &[String]) -> anyhow::Result<Vec<CommandArg>> {
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            let bytes_arg = matches!(command_name, "handset-command" | "ble-gadget-write")
                && index == args.len() - 1
                || command_name == "xcp-command" && index == 1;
            if bytes_arg {
                parse_hex(arg).map(CommandArg::Bytes)
            } else if let Some(value) = parse_number(arg) {
                Ok(CommandArg::Number(value))
            } else {
                Ok(CommandArg::Text(arg.clone()))
            }
        })
        .collect::<Result<Vec<_>, _>>()
}

fn parse_number(value: &str) -> Option<u64> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else if value.chars().all(|ch| ch.is_ascii_digit()) {
        value.parse().ok()
    } else {
        None
    }
}

fn parse_hex(value: &str) -> anyhow::Result<Vec<u8>> {
    let normalized = value
        .strip_prefix("hex:")
        .unwrap_or(value)
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '\t' | '\n' | '\r' | ':' | '_' | '-'))
        .collect::<String>();
    Ok(hex::decode(normalized)?)
}

fn parse_csv_u64(value: &str) -> anyhow::Result<Vec<u64>> {
    let values = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse::<u64>()
                .map_err(|error| anyhow::anyhow!("invalid integer '{item}': {error}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if values.is_empty() {
        anyhow::bail!("at least one interval is required");
    }
    Ok(values)
}

fn latency_summary(values: &[u128]) -> serde_json::Value {
    if values.is_empty() {
        return json!(null);
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let sum = sorted.iter().sum::<u128>() as f64;
    json!({
        "min": sorted[0],
        "p50": percentile_nearest_rank(&sorted, 50),
        "p90": percentile_nearest_rank(&sorted, 90),
        "max": sorted[sorted.len() - 1],
        "avg": sum / sorted.len() as f64,
    })
}

fn percentile_nearest_rank(sorted: &[u128], percentile: usize) -> u128 {
    let rank = (percentile * sorted.len()).div_ceil(100).max(1);
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_height_target_uses_centimetres() {
        assert_eq!(height_units_from_cm(62.0).unwrap(), 620);
        assert_eq!(height_units_from_cm(100.0).unwrap(), 1000);
        assert_eq!(height_units_from_cm(72.5).unwrap(), 725);
        assert!(height_units_from_cm(0.0).is_err());
    }

    #[test]
    fn height_delta_allows_signed_centimetres() {
        assert_eq!(height_delta_units_from_cm(5.0).unwrap(), 50);
        assert_eq!(height_delta_units_from_cm(-2.5).unwrap(), -25);
        assert!(height_delta_units_from_cm(0.0).is_err());
        assert!(height_delta_units_from_cm(0.01).is_err());
    }

    #[test]
    fn adjust_height_accepts_negative_delta() {
        let cli = Cli::try_parse_from([
            "desk",
            "adjust-height",
            "--device-name",
            "LOGIClink C1022",
            "-5",
        ])
        .unwrap();
        let Command::AdjustHeight { delta_cm, .. } = cli.command else {
            panic!("expected adjust-height command");
        };
        assert_eq!(delta_cm, -5.0);
    }

    #[test]
    fn motion_events_use_notification_receive_timestamps() {
        let started_at = std::time::Instant::now();
        let mut events = Vec::new();
        let mut last_height = None;

        collect_motion_events(
            &mut events,
            vec![
                json!({
                    "receivedAtMs": 125,
                    "payload": "0102",
                    "parsed": {
                        "height": 1000,
                        "movementCounter": 7
                    }
                }),
                json!({
                    "receivedAtMs": 225,
                    "payload": "0304",
                    "parsed": {
                        "height": 1004,
                        "movementCounter": 8
                    }
                }),
            ],
            started_at,
            &mut last_height,
            Some(2),
        );

        assert_eq!(events[0]["atMs"], json!(125_u128));
        assert_eq!(events[0]["height"], json!(1000));
        assert_eq!(events[0]["movementCounter"], json!(7));
        assert_eq!(events[1]["atMs"], json!(225_u128));
        assert_eq!(events[1]["derivedSpeedUnitsPerSecond"], json!(40.0));
    }

    #[test]
    fn motion_analysis_reports_write_to_height_change_latency() {
        let writes = vec![
            json!({"tick": 1, "atMs": 10}),
            json!({"tick": 2, "atMs": 110}),
        ];
        let events = vec![
            json!({"atMs": 80, "tick": 1, "height": 1000}),
            json!({"atMs": 180, "tick": 2, "height": 1005}),
        ];

        let analysis = motion_analysis(Some(1000), Some(1008), &writes, &events);

        assert_eq!(analysis["heightChangeLatencyMs"], json!(170));
        assert_eq!(analysis["firstHeightChangeTick"], json!(2));
        assert_eq!(analysis["finalHeightMinusLastNotification"], json!(3));
    }
}
