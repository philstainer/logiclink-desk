# logiclink-desk

CLI tooling for LOGIClink Bluetooth standing desks.

## NPM usage

This package is intended to ship prebuilt Rust binaries, so users do not need a Rust toolchain installed.

```sh
npx logiclink-desk height
npx logiclink-desk set-height 100
```

## Publishing

Build and stage the current platform binary before packing or publishing:

```sh
npm run prepare:binary
npm pack
```

The NPM CLI entrypoint looks for binaries in `prebuilt/<platform>-<arch>/logiclink-desk`, for example `prebuilt/darwin-arm64/logiclink-desk`.
