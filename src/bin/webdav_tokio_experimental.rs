use std::{env, net::SocketAddr};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{Duration, timeout},
};
use webdav_wasi::{
    FileSystemBackend, MemoryBackend, WebDavBackend, WebDavFileSystem, serve, serve_listener,
};

#[derive(Debug)]
struct Args {
    addr: SocketAddr,
    smoke_test: bool,
    fs_root: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = parse_args()?;

    if args.smoke_test {
        return run_smoke_test(args.fs_root.as_deref()).await;
    }

    run_server(args.addr, args.fs_root.as_deref()).await
}

fn parse_args() -> anyhow::Result<Args> {
    let mut args = env::args().skip(1);
    let mut parsed = Args {
        addr: "127.0.0.1:8080".parse().unwrap(),
        smoke_test: false,
        fs_root: None,
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke-test" => parsed.smoke_test = true,
            "--addr" => {
                parsed.addr = args
                    .next()
                    .context("missing socket address after --addr")?
                    .parse()
                    .context("invalid socket address")?;
            }
            "--fs-root" => {
                parsed.fs_root = Some(args.next().context("missing path after --fs-root")?);
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    Ok(parsed)
}

async fn run_server(addr: SocketAddr, fs_root: Option<&str>) -> anyhow::Result<()> {
    match fs_root {
        Some(root) => {
            let backend = FileSystemBackend::new(root)
                .with_context(|| format!("failed to initialize file backend at {root}"))?;
            log::info!(
                "serving filesystem backend from {}",
                backend.root().display()
            );
            serve(addr, WebDavFileSystem::new(backend)).await
        }
        None => {
            let backend = MemoryBackend::demo();
            serve(addr, WebDavFileSystem::new(backend)).await
        }
    }
}

async fn run_smoke_test(fs_root: Option<&str>) -> anyhow::Result<()> {
    match fs_root {
        Some(root) => {
            let backend = FileSystemBackend::new(root)
                .with_context(|| format!("failed to initialize file backend at {root}"))?;
            ensure_smoke_test_fixture(&backend).await?;
            run_smoke_test_with_backend(backend).await
        }
        None => run_smoke_test_with_backend(MemoryBackend::demo()).await,
    }
}

async fn run_smoke_test_with_backend<B>(backend: B) -> anyhow::Result<()>
where
    B: WebDavBackend,
{
    let filesystem = WebDavFileSystem::new(backend);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind smoke-test listener")?;
    let addr = listener
        .local_addr()
        .context("failed to get smoke-test addr")?;

    let server = tokio::spawn(async move { serve_listener(listener, filesystem).await });
    let result = smoke_get(addr).await;
    server.abort();
    result
}

async fn ensure_smoke_test_fixture<B>(backend: &B) -> anyhow::Result<()>
where
    B: WebDavBackend,
{
    backend
        .write_chunk("hello.txt", 0, b"hello from webdav-wasi\n".to_vec())
        .await
        .context("failed to seed smoke-test file")?;
    backend
        .truncate("hello.txt", "hello from webdav-wasi\n".len() as u64)
        .await
        .context("failed to size smoke-test file")?;
    Ok(())
}

async fn smoke_get(addr: SocketAddr) -> anyhow::Result<()> {
    let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(addr))
        .await
        .context("timed out connecting to webdav server")??;

    stream
        .write_all(b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .context("failed to write request")?;
    stream.flush().await.context("failed to flush request")?;

    let mut response = Vec::new();
    timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .context("timed out reading response")??;

    let response = String::from_utf8(response).context("response was not valid utf-8")?;
    if !response.starts_with("HTTP/1.1 200") {
        bail!("unexpected response status: {response}");
    }
    if !response.contains("hello from webdav-wasi") {
        bail!("unexpected response body: {response}");
    }

    println!("smoke test passed against http://{addr}/hello.txt");
    Ok(())
}
