# webdav-wasi

Standalone WebDAV WASIp2 experiment extracted from [fungi](https://github.com/enbop/fungi)'s legacy file transfer WebDAV module.

The component is a long-running `wasi:cli/run` application. It owns one Tokio
runtime, TCP listener, HTTP server, and WebDAV backend for its full lifetime.

## What This Is

- A small `dav-server::fs::DavFileSystem` adapter derived from [fungi](https://github.com/enbop/fungi)'s `webdav_impl.rs`.
- A local `WebDavBackend` trait replacing [fungi](https://github.com/enbop/fungi)'s `FileTransferClientsControl` RPC backend.
- Two demo backends:
  - `FileSystemBackend`: serves a WASI-preopened directory through `std::fs`.
  - `MemoryBackend`: in-memory demo files.

## Build

```bash
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2 --bin webdav-wasi
```

## Run With Wasmtime

Serve the local `data` directory:

```bash
mkdir -p data
wasmtime run -Scli -Stcp -Sinherit-network --dir ./data::data \
  target/wasm32-wasip2/release/webdav-wasi.wasm \
  --addr 127.0.0.1:8080 --fs-root data
```

Then open or mount:

```text
http://localhost:8080/
```

Use a different guest-visible root with `WEBDAV_FS_ROOT`:

```bash
mkdir -p shared
wasmtime run -Scli -Stcp -Sinherit-network --env WEBDAV_FS_ROOT=shared \
  --dir ./shared::shared target/wasm32-wasip2/release/webdav-wasi.wasm \
  --addr 127.0.0.1:8080
```

If no filesystem root is detected, the app falls back to the in-memory demo backend.

Native smoke test (always uses isolated in-memory data):

```bash
cargo run --bin webdav-wasi -- --smoke-test
```

The smoke test ignores `--fs-root`, `WEBDAV_FS_ROOT`, and any local `data/`
directory. It checks the listener and HTTP handler without creating or changing
service files. To test the filesystem backend, start the server with a disposable
`--fs-root` directory and exercise file operations through WebDAV.

Native server:

```bash
cargo run --bin webdav-wasi -- --addr 127.0.0.1:8080 --fs-root ./data
```

The repository's `.cargo/config.toml` enables Tokio's unstable WASIp2 network
support for native and component builds.

## Notes

- All requests in one service process share the same backend state.
- File IO currently uses synchronous `std::fs` through WASI filesystem hostcalls.
  Long filesystem operations can block other requests on the shared current-thread
  runtime; this remains an accepted limitation of this experimental backend.
- The guest can only access directories preopened with `--dir`.
- WebDAV properties are currently no-op, matching the minimal [fungi](https://github.com/enbop/fungi) extraction.
- Client compatibility still needs real-world testing with Finder, Windows WebDAV, Cyberduck, rclone, and similar clients.
