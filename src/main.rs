use std::{env, path::Path, sync::LazyLock};

use dav_server::{DavHandler, body::Body as DavBody, fakels::FakeLs};
use http::{Method, header::CONTENT_LENGTH};
use http_body_util::BodyExt;
use webdav_wasi::{FileSystemBackend, MemoryBackend, WebDavFileSystem};
use wstd::http::{Body, Request, Response, StatusCode};

static DAV_HANDLER: LazyLock<Result<DavHandler, String>> = LazyLock::new(init_handler);

#[wstd::http_server]
async fn main(request: Request<Body>) -> Result<Response<Body>, wstd::http::Error> {
    match DAV_HANDLER.as_ref() {
        Ok(handler) => {
            let method = request.method().clone();
            Ok(convert_response(
                method,
                handle_request(handler, request).await,
            ))
        }
        Err(message) => Ok(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            message.as_str(),
        )),
    }
}

async fn handle_request(handler: &DavHandler, request: Request<Body>) -> Response<DavBody> {
    let (parts, body) = request.into_parts();
    let body = body
        .into_boxed_body()
        .map_err(|error| std::io::Error::other(error.to_string()));
    handler.handle(Request::from_parts(parts, body)).await
}

fn init_handler() -> Result<DavHandler, String> {
    let _ = env_logger::try_init();

    let builder = DavHandler::builder().locksystem(FakeLs::new());

    if let Some(root) = detect_fs_root() {
        let backend = FileSystemBackend::new(&root)
            .map_err(|error| format!("failed to initialize file backend at {root}: {error}"))?;
        log::info!(
            "wasmtime serve mode using filesystem backend at {}",
            backend.root().display()
        );
        return Ok(builder
            .filesystem(Box::new(WebDavFileSystem::new(backend)))
            .build_handler());
    }

    log::info!("wasmtime serve mode using in-memory demo backend");
    Ok(builder
        .filesystem(Box::new(WebDavFileSystem::new(MemoryBackend::demo())))
        .build_handler())
}

fn detect_fs_root() -> Option<String> {
    if let Ok(root) = env::var("WEBDAV_FS_ROOT") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if Path::new("data").exists() {
        return Some("data".to_string());
    }

    None
}

fn convert_response(method: Method, response: Response<DavBody>) -> Response<Body> {
    let (mut parts, body) = response.into_parts();
    if method == Method::HEAD || response_status_has_no_body(parts.status) {
        parts.headers.remove(CONTENT_LENGTH);
    }
    let body = body.map_err(|error| std::io::Error::other(error.to_string()));
    Response::from_parts(parts, Body::from_http_body(body))
}

fn response_status_has_no_body(status: StatusCode) -> bool {
    status.is_informational()
        || status == StatusCode::NO_CONTENT
        || status == StatusCode::RESET_CONTENT
        || status == StatusCode::NOT_MODIFIED
}

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Body::from(message.to_owned()))
        .expect("error response should build")
}
