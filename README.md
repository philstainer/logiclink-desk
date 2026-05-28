# logiclink-desk

CLI tooling for controlling LOGIClink Bluetooth standing desks.

This project is a Rust command-line application with an optional npm wrapper. It can read the current desk height, send movement commands, move to a target height, and inspect or build the underlying protocol packets used by the desk.

## Safety warning

Use this software at your own risk.

This tool sends Bluetooth commands that can make a motorised desk move up or down. Before running any movement command, make sure the area above, below, and around the desk is clear. Keep people, pets, cables, monitors, shelves, chairs, and other objects away from moving parts.

The software cannot guarantee that the desk will stop before hitting an obstruction, reaching a limit, or moving farther than expected. Stay near the desk while it is moving and be ready to stop it using the desk's physical controls or by cutting power if needed.

Do not use this tool unattended, remotely, or in any situation where unexpected desk movement could cause injury or damage.

## Requirements

- A LOGIClink Bluetooth standing desk.
- Bluetooth enabled on the machine running the CLI.
- Rust toolchain for local development and building from source.
- Node.js 18 or newer if using the npm wrapper or packaging scripts.

Bluetooth permissions vary by operating system. You may need to allow terminal, shell, or Node/Rust processes to access Bluetooth.

## Install dependencies

```sh
cargo fetch
npm install
```

`npm install` is only required for the npm packaging flow. The Rust CLI itself builds with Cargo.

## Run from source

Read the current desk height:

```sh
cargo run -- height
```

Move to a target height in centimetres:

```sh
cargo run -- set-height 100
```

Raise or lower by a relative amount in centimetres:

```sh
cargo run -- raise 5
cargo run -- lower 2.5
```

If more than one compatible device is visible, pass a device name:

```sh
cargo run -- height --device-name "Your Desk Name"
```

Print all commands:

```sh
cargo run -- --help
```

Print help for a specific command:

```sh
cargo run -- set-height --help
```

## Build

Build a debug binary:

```sh
cargo build
```

Build an optimised release binary:

```sh
cargo build --release
```

The release binary is written to:

```sh
target/release/logiclink-desk
```

Run it directly:

```sh
./target/release/logiclink-desk height
```

## NPM usage

The npm package is intended to ship prebuilt Rust binaries, so users do not need a Rust toolchain installed.

```sh
npx logiclink-desk height
npx logiclink-desk set-height 100
```

The npm CLI entrypoint looks for binaries in `prebuilt/<platform>-<arch>/logiclink-desk`, for example:

```sh
prebuilt/darwin-arm64/logiclink-desk
```

## Packaging

Build and stage the current platform binary before packing or publishing:

```sh
npm run prepare:binary
npm pack
```

The GitHub Actions workflow publishes to npm whenever changes are pushed to `main`.
Add an npm automation token as the repository secret `NPM_TOKEN` before relying on the workflow.

Available npm scripts:

```sh
npm run build:release
npm run package:binary
npm run prepare:binary
```

## Useful commands

Read-only commands:

```sh
logiclink-desk height
logiclink-desk query get-height
logiclink-desk decode <hex>
logiclink-desk build <command>
```

Movement commands:

```sh
logiclink-desk set-height 100
logiclink-desk adjust-height -2.5
logiclink-desk raise 5
logiclink-desk lower 5
logiclink-desk pulse up 5
logiclink-desk burst down 20
```

Profiling and diagnostics:

```sh
logiclink-desk watch-motion up 20
logiclink-desk profile-motion down 10
logiclink-desk bench-height-poll
```

## Development

Run the test suite:

```sh
cargo test
```

Check formatting:

```sh
cargo fmt --check
```

Run Clippy:

```sh
cargo clippy --all-targets --all-features
```
