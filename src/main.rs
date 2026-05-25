use std::time::Duration;

use bluetooth_desk_control_rust::ble::{DeskSession, QueryOptions, run_query};
use bluetooth_desk_control_rust::commands::{CommandArg, command_payload};
use bluetooth_desk_control_rust::protocol::{
    decode_packet, decode_slip_stream, encode_packet, resolve_characteristic, slip_encode,
};
use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(version, about = "Rust LOGIClink Bluetooth desk control tooling")]
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
        #[arg(default_value = "LOGIClink C1022")]
        target_name: String,
        #[arg(default_value = "c1:02:2a:05:47:b1")]
        target_address: String,
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
        /// Allow drive-up, drive-down, or drive-to. This physically moves the desk.
        #[arg(long)]
        i_understand_this_moves_the_desk: bool,
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
        #[arg(default_value = "LOGIClink C1022")]
        target_name: String,
        #[arg(default_value = "c1:02:2a:05:47:b1")]
        target_address: String,
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
        /// Allow physical desk movement.
        #[arg(long)]
        i_understand_this_moves_the_desk: bool,
    },
    /// Send repeated motion ticks without per-tick height polling, then read final height.
    Burst {
        #[arg(default_value = "up")]
        direction: String,
        #[arg(default_value_t = 20)]
        ticks: u16,
        #[arg(default_value = "LOGIClink C1022")]
        target_name: String,
        #[arg(default_value = "c1:02:2a:05:47:b1")]
        target_address: String,
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
        /// Allow physical desk movement.
        #[arg(long)]
        i_understand_this_moves_the_desk: bool,
    },
    /// Move toward a target height using continuous jog ticks and live motion feedback.
    SetHeight {
        target_height: i64,
        #[arg(default_value = "LOGIClink C1022")]
        target_name: String,
        #[arg(default_value = "c1:02:2a:05:47:b1")]
        target_address: String,
        #[arg(default_value = "app")]
        characteristic: String,
        #[arg(long, default_value_t = 3)]
        tolerance: i64,
        #[arg(long, default_value_t = 10)]
        burst_ticks: u16,
        #[arg(long, default_value_t = 180)]
        interval_ms: u64,
        #[arg(long, default_value_t = 3)]
        fine_ticks: u16,
        #[arg(long, default_value_t = 150)]
        fine_interval_ms: u64,
        #[arg(long, default_value_t = 40)]
        fine_band: i64,
        #[arg(long, default_value_t = 18)]
        coast_margin: i64,
        #[arg(long, default_value_t = 30)]
        up_coast_margin: i64,
        #[arg(long, default_value_t = 16)]
        down_coast_margin: i64,
        #[arg(long, default_value_t = 250)]
        feedback_lag_ms: u64,
        #[arg(long, default_value_t = 150)]
        settle_ms: u64,
        #[arg(long, default_value_t = 150)]
        correction_settle_ms: u64,
        #[arg(long, default_value_t = 50)]
        height_poll_ms: u64,
        #[arg(long, default_value_t = 0)]
        reversal_settle_ms: u64,
        #[arg(long, default_value_t = 60_000)]
        timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        scan_timeout_ms: u64,
        #[arg(long, default_value_t = 15_000)]
        connect_timeout_ms: u64,
        #[arg(long, default_value_t = 150)]
        response_window_ms: u64,
        /// Allow physical desk movement.
        #[arg(long)]
        i_understand_this_moves_the_desk: bool,
    },
    /// Stream jog ticks and print live height notifications with derived speed.
    WatchMotion {
        #[arg(default_value = "up")]
        direction: String,
        #[arg(default_value_t = 20)]
        ticks: u16,
        #[arg(default_value = "LOGIClink C1022")]
        target_name: String,
        #[arg(default_value = "c1:02:2a:05:47:b1")]
        target_address: String,
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
        /// Allow physical desk movement.
        #[arg(long)]
        i_understand_this_moves_the_desk: bool,
    },
    /// Benchmark connected get-height response latency at one or more requested intervals.
    BenchHeightPoll {
        #[arg(default_value = "LOGIClink C1022")]
        target_name: String,
        #[arg(default_value = "c1:02:2a:05:47:b1")]
        target_address: String,
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
            target_name,
            target_address,
            query,
            characteristic,
            args,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
            i_understand_this_moves_the_desk,
        } => {
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let args = parse_command_args(&query, &args)?;
            let result = run_query(QueryOptions {
                target_name,
                target_address: target_address.to_ascii_lowercase(),
                query,
                args,
                characteristic,
                scan_timeout: Duration::from_millis(scan_timeout_ms),
                connect_timeout: Duration::from_millis(connect_timeout_ms),
                response_window: Duration::from_millis(response_window_ms),
                allow_motion: i_understand_this_moves_the_desk,
            })
            .await?;
            print_json(result)?;
        }
        Command::Pulse {
            direction,
            ticks,
            target_height,
            target_name,
            target_address,
            characteristic,
            interval_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
            i_understand_this_moves_the_desk,
        } => {
            if !i_understand_this_moves_the_desk {
                anyhow::bail!(
                    "pulse physically moves the desk; pass --i-understand-this-moves-the-desk"
                );
            }
            let motion_name = motion_command_name(&direction)?;
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let mut samples = Vec::new();
            let response_window = Duration::from_millis(response_window_ms);

            let mut session = DeskSession::connect(query_options(
                &target_name,
                &target_address,
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
                "targetName": target_name,
                "targetAddress": target_address,
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
            target_name,
            target_address,
            characteristic,
            interval_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
            i_understand_this_moves_the_desk,
        } => {
            if !i_understand_this_moves_the_desk {
                anyhow::bail!(
                    "burst physically moves the desk; pass --i-understand-this-moves-the-desk"
                );
            }
            let motion_name = motion_command_name(&direction)?;
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let response_window = Duration::from_millis(response_window_ms);
            let mut session = DeskSession::connect(query_options(
                &target_name,
                &target_address,
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
                "targetName": target_name,
                "targetAddress": target_address,
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
            target_height,
            target_name,
            target_address,
            characteristic,
            tolerance,
            burst_ticks,
            interval_ms,
            fine_ticks,
            fine_interval_ms,
            fine_band,
            coast_margin,
            up_coast_margin,
            down_coast_margin,
            feedback_lag_ms,
            settle_ms,
            correction_settle_ms,
            height_poll_ms,
            reversal_settle_ms,
            timeout_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
            i_understand_this_moves_the_desk,
        } => {
            if !i_understand_this_moves_the_desk {
                anyhow::bail!(
                    "set-height physically moves the desk; pass --i-understand-this-moves-the-desk"
                );
            }
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let response_window = Duration::from_millis(response_window_ms);
            let mut session = DeskSession::connect(query_options(
                &target_name,
                &target_address,
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
            let mut current = read_height(&mut session, response_window).await?;
            let starting_height = current;
            samples.push(json!({
                "label": "start",
                "height": current,
                "deltaToTarget": target_height - current,
            }));

            let mut correction_count = 0_u32;
            let mut previous_direction: Option<&'static str> = None;
            while (target_height - current).abs() > tolerance {
                if started_at.elapsed() >= Duration::from_millis(timeout_ms) {
                    anyhow::bail!(
                        "timed out moving to target: current={current} target={target_height}"
                    );
                }

                let delta = target_height - current;
                let direction = if delta > 0 { "drive-up" } else { "drive-down" };
                let remaining = delta.abs();
                let fine_mode = remaining <= fine_band || correction_count > 0;
                let direction_coast_margin = match direction {
                    "drive-up" => up_coast_margin,
                    "drive-down" => down_coast_margin,
                    _ => coast_margin,
                };
                let feedback_stop_margin = stop_margin_for_feedback_lag(
                    speed_units_per_second(direction),
                    feedback_lag_ms,
                );
                let base_stop_margin = if fine_mode {
                    match direction {
                        "drive-up" => direction_coast_margin
                            .max(tolerance)
                            .max(feedback_stop_margin),
                        _ => (direction_coast_margin / 3)
                            .max(tolerance)
                            .max(feedback_stop_margin / 2),
                    }
                } else {
                    direction_coast_margin
                        .max(tolerance)
                        .max(feedback_stop_margin)
                };
                let tick_interval = if fine_mode {
                    fine_interval_ms
                } else {
                    interval_ms
                };
                let mut tick_count = 0_u32;
                let mut live_heights = Vec::new();
                let mut height_polls = 0_u32;
                let previous_height = current;
                let speed_units_per_second = speed_units_per_second(direction);
                let units_per_tick =
                    speed_units_per_second * Duration::from_millis(tick_interval).as_secs_f64();
                let planned_ticks = if fine_mode {
                    ((remaining.saturating_sub(tolerance)) as f64 / units_per_tick)
                        .ceil()
                        .max(1.0) as u32
                } else {
                    let planned_distance = (remaining - base_stop_margin).max(1) as f64;
                    ((planned_distance / speed_units_per_second * 1000.0) / tick_interval as f64)
                        .ceil()
                        .max(1.0) as u32
                };
                let planned_ticks = if fine_mode {
                    planned_ticks.min(u32::from(fine_ticks.max(1)))
                } else {
                    planned_ticks
                };
                let stop_when_within = if fine_mode && direction == "drive-down" {
                    tolerance
                } else {
                    base_stop_margin
                };

                session.drain_notifications(Duration::from_millis(50)).await;

                let reversing = previous_direction.is_some_and(|previous| previous != direction);
                if reversing {
                    tokio::time::sleep(Duration::from_millis(reversal_settle_ms)).await;
                }

                drive_and_poll_height(
                    &mut session,
                    direction,
                    planned_ticks,
                    Duration::from_millis(tick_interval),
                    Duration::from_millis(height_poll_ms),
                    response_window,
                    target_height,
                    stop_when_within,
                    started_at,
                    &mut current,
                    &mut live_heights,
                    &mut tick_count,
                    &mut height_polls,
                    Duration::from_millis(timeout_ms),
                )
                .await?;

                previous_direction = Some(direction);
                let settle_after_move = if correction_count > 0 {
                    correction_settle_ms
                } else {
                    settle_ms
                };
                current = update_cached_height_during_wait(
                    &mut session,
                    Duration::from_millis(settle_after_move),
                    Duration::from_millis(height_poll_ms),
                    response_window,
                    started_at,
                    current,
                    &mut live_heights,
                    &mut height_polls,
                )
                .await?;
                if (target_height - current).signum() != (target_height - previous_height).signum()
                    && (target_height - current).abs() > tolerance
                {
                    correction_count += 1;
                }
                samples.push(json!({
                    "label": format!("step-{}", samples.len()),
                    "direction": direction,
                    "ticks": tick_count,
                    "plannedTicks": planned_ticks,
                    "stopWhenWithin": stop_when_within,
                    "heightPolls": height_polls,
                    "burstTicks": burst_ticks,
                    "fineTicks": fine_ticks,
                    "unitsPerTick": units_per_tick,
                    "intervalMs": tick_interval,
                    "baseStopMargin": base_stop_margin,
                    "feedbackStopMargin": feedback_stop_margin,
                    "feedbackLagMs": feedback_lag_ms,
                    "fineMode": fine_mode,
                    "reversing": reversing,
                    "speedUnitsPerSecond": speed_units_per_second,
                    "height": current,
                    "deltaToTarget": target_height - current,
                    "motionHeights": live_heights,
                    "correctionCount": correction_count,
                    "settleMs": settle_after_move,
                    "heightPollMs": height_poll_ms,
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
                "targetName": target_name,
                "targetAddress": target_address,
                "action": "set-height",
                "targetHeight": target_height,
                "tolerance": tolerance,
                "startingHeight": starting_height,
                "finalHeight": current,
                "observedDelta": current - starting_height,
                "withinTolerance": (target_height - current).abs() <= tolerance,
                "elapsedMs": started_at.elapsed().as_millis(),
                "coastMargin": coast_margin,
                "upCoastMargin": up_coast_margin,
                "downCoastMargin": down_coast_margin,
                "feedbackLagMs": feedback_lag_ms,
                "settleMs": settle_ms,
                "correctionSettleMs": correction_settle_ms,
                "heightPollMs": height_poll_ms,
                "reversalSettleMs": reversal_settle_ms,
                "samples": samples,
            }))?;
        }
        Command::WatchMotion {
            direction,
            ticks,
            target_name,
            target_address,
            characteristic,
            interval_ms,
            drain_ms,
            scan_timeout_ms,
            connect_timeout_ms,
            response_window_ms,
            i_understand_this_moves_the_desk,
        } => {
            if !i_understand_this_moves_the_desk {
                anyhow::bail!(
                    "watch-motion physically moves the desk; pass --i-understand-this-moves-the-desk"
                );
            }
            let motion_name = motion_command_name(&direction)?;
            let characteristic = resolve_characteristic(&characteristic)?.to_string();
            let response_window = Duration::from_millis(response_window_ms);
            let mut session = DeskSession::connect(query_options(
                &target_name,
                &target_address,
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
                "targetName": target_name,
                "targetAddress": target_address,
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
            target_name,
            target_address,
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
                &target_name,
                &target_address,
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
                "targetName": target_name,
                "targetAddress": target_address,
                "action": "bench-height-poll",
                "connectedElapsedMs": connected_at.elapsed().as_millis(),
                "responseWaitMs": response_wait_ms,
                "responseQuietMs": response_quiet_ms,
                "runs": runs,
            }))?;
        }
    }

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

fn query_options(
    target_name: &str,
    target_address: &str,
    query: &str,
    args: Vec<CommandArg>,
    characteristic: &str,
    scan_timeout_ms: u64,
    connect_timeout_ms: u64,
    response_window_ms: u64,
    allow_motion: bool,
) -> QueryOptions {
    QueryOptions {
        target_name: target_name.to_string(),
        target_address: target_address.to_ascii_lowercase(),
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

fn speed_units_per_second(direction: &str) -> f64 {
    match direction {
        "drive-up" => 34.0,
        "drive-down" => 34.0,
        _ => 34.0,
    }
}

fn stop_margin_for_feedback_lag(speed_units_per_second: f64, feedback_lag_ms: u64) -> i64 {
    (speed_units_per_second * feedback_lag_ms as f64 / 1000.0).ceil() as i64
}

#[allow(clippy::too_many_arguments)]
async fn drive_and_poll_height(
    session: &mut DeskSession,
    direction: &str,
    planned_ticks: u32,
    tick_interval: Duration,
    height_poll_interval: Duration,
    height_response_timeout: Duration,
    target_height: i64,
    stop_when_within: i64,
    started_at: std::time::Instant,
    current: &mut i64,
    live_heights: &mut Vec<i64>,
    tick_count: &mut u32,
    height_polls: &mut u32,
    timeout_after: Duration,
) -> anyhow::Result<()> {
    let mut next_tick_at = tokio::time::Instant::now();
    let mut next_poll_at = tokio::time::Instant::now() + height_poll_interval;
    let mut height_request_sent_at: Option<tokio::time::Instant> = None;

    while *tick_count < planned_ticks && (target_height - *current).abs() > stop_when_within {
        if started_at.elapsed() >= timeout_after {
            anyhow::bail!("timed out moving to target: current={current} target={target_height}");
        }

        let now = tokio::time::Instant::now();
        if height_request_sent_at.is_some_and(|sent_at| now >= sent_at + height_response_timeout) {
            height_request_sent_at = None;
            next_poll_at = now;
        }

        if now >= next_tick_at {
            session
                .write_command(direction, &[CommandArg::Number(1)])
                .await?;
            *tick_count += 1;
            next_tick_at = now + tick_interval;
        }

        if height_request_sent_at.is_none() && now >= next_poll_at {
            session.write_command("get-height", &[]).await?;
            *height_polls += 1;
            height_request_sent_at = Some(now);
            next_poll_at = now + height_poll_interval;
        }

        if update_height_from_available_notifications(session, started_at, current, live_heights)
            .await
            > 0
        {
            height_request_sent_at = None;
        }

        let next_height_wake = height_request_sent_at
            .map(|sent_at| sent_at + height_response_timeout)
            .unwrap_or(next_poll_at);
        let sleep_until = next_tick_at.min(next_height_wake);
        let now = tokio::time::Instant::now();
        if sleep_until > now {
            tokio::time::sleep((sleep_until - now).min(Duration::from_millis(10))).await;
        }
    }

    update_height_from_available_notifications(session, started_at, current, live_heights).await;
    Ok(())
}

async fn update_cached_height_during_wait(
    session: &mut DeskSession,
    duration: Duration,
    height_poll_interval: Duration,
    height_response_timeout: Duration,
    started_at: std::time::Instant,
    mut current: i64,
    live_heights: &mut Vec<i64>,
    height_polls: &mut u32,
) -> anyhow::Result<i64> {
    let deadline = tokio::time::Instant::now() + duration;
    let mut next_poll_at = tokio::time::Instant::now();
    let starting_height_count = live_heights.len();
    let mut height_request_sent_at: Option<tokio::time::Instant> = None;

    while tokio::time::Instant::now() < deadline {
        let now = tokio::time::Instant::now();
        if height_request_sent_at.is_some_and(|sent_at| now >= sent_at + height_response_timeout) {
            height_request_sent_at = None;
            next_poll_at = now;
        }

        if height_request_sent_at.is_none() && now >= next_poll_at {
            session.write_command("get-height", &[]).await?;
            *height_polls += 1;
            height_request_sent_at = Some(now);
            next_poll_at = now + height_poll_interval;
        }

        if update_height_from_available_notifications(
            session,
            started_at,
            &mut current,
            live_heights,
        )
        .await
            > 0
        {
            height_request_sent_at = None;
        }
        if live_heights.len() > starting_height_count {
            return Ok(current);
        }

        let now = tokio::time::Instant::now();
        let next_height_wake = height_request_sent_at
            .map(|sent_at| sent_at + height_response_timeout)
            .unwrap_or(next_poll_at);
        let sleep_until = next_height_wake.min(deadline);
        if sleep_until > now {
            tokio::time::sleep((sleep_until - now).min(Duration::from_millis(10))).await;
        }
    }

    update_height_from_available_notifications(session, started_at, &mut current, live_heights)
        .await;
    if live_heights.len() > starting_height_count {
        return Ok(current);
    }

    Ok(current)
}

async fn update_height_from_available_notifications(
    session: &mut DeskSession,
    started_at: std::time::Instant,
    current: &mut i64,
    live_heights: &mut Vec<i64>,
) -> usize {
    let notifications = session
        .drain_available_notifications_timed(started_at)
        .await;
    let mut updated = 0;
    for height in notifications.iter().filter_map(notification_height) {
        *current = height;
        live_heights.push(height);
        updated += 1;
    }
    updated
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
    Ok(args
        .iter()
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
        .collect::<Result<Vec<_>, _>>()?)
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
    fn feedback_lag_margin_rounds_up_movement_during_lag() {
        assert_eq!(stop_margin_for_feedback_lag(34.0, 250), 9);
        assert_eq!(stop_margin_for_feedback_lag(34.0, 0), 0);
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
