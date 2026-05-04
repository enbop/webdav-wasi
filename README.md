# webdav-wasip2

Standalone WebDAV WASIp2 experiment extracted from [fungi](https://github.com/enbop/fungi)'s legacy file transfer WebDAV module.

The main runtime path is `wasmtime serve`: Wasmtime owns the HTTP listener, and the guest component handles WebDAV requests.

## What This Is

- A small `dav-server::fs::DavFileSystem` adapter derived from [fungi](https://github.com/enbop/fungi)'s `webdav_impl.rs`.
- A local `WebDavBackend` trait replacing [fungi](https://github.com/enbop/fungi)'s `FileTransferClientsControl` RPC backend.
- Two demo backends:
  - `FileSystemBackend`: serves a WASI-preopened directory through `std::fs`.
  - `MemoryBackend`: in-memory demo files.

## Build

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --no-default-features --features wasmtime-serve --bin webdav-wasip2
```

## Run With Wasmtime

Serve the local `data` directory:

```bash
mkdir -p data
wasmtime serve --addr=0.0.0.0:8080 -Scli --dir ./data::data \
  target/wasm32-wasip2/debug/webdav-wasip2.wasm
```

Then open or mount:

```text
http://localhost:8080/
```

Use a different guest-visible root with `WEBDAV_FS_ROOT`:

```bash
mkdir -p shared
WEBDAV_FS_ROOT=shared wasmtime serve --addr=0.0.0.0:8080 -Scli --dir ./shared::shared \
  target/wasm32-wasip2/debug/webdav-wasip2.wasm
```

If no filesystem root is detected, the app falls back to the in-memory demo backend.

## Experimental Tokio Server

There is also a guest-owned TCP/HTTP server for comparison. This is not the preferred WASI path for WebDAV.

Native smoke test:

```bash
cargo run --features tokio-server --bin webdav-wasip2-tokio-experimental -- --smoke-test
```

Native server:

```bash
cargo run --features tokio-server --bin webdav-wasip2-tokio-experimental -- --addr 127.0.0.1:8080 --fs-root ./data
```

WASIp2 Tokio builds require Tokio's unstable WASI net support:

```bash
RUSTFLAGS="--cfg tokio_unstable" cargo build --target wasm32-wasip2 --features tokio-server --bin webdav-wasip2-tokio-experimental
```

## Notes

- `wasmtime serve` + `wstd` is the primary direction for this WebDAV experiment.
- The Tokio server is kept only as a comparison path for guest-owned TCP.
- File IO currently uses synchronous `std::fs` through WASI filesystem hostcalls.
- The guest can only access directories preopened with `--dir`.
- WebDAV properties are currently no-op, matching the minimal [fungi](https://github.com/enbop/fungi) extraction.
- Client compatibility still needs real-world testing with Finder, Windows WebDAV, Cyberduck, rclone, and similar clients.
