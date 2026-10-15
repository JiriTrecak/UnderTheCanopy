//! The updater's network thread: HTTPS requests on a single-threaded tokio runtime with
//! the platform's certificate verifier (the multiplayer client's TLS configuration).
//! Callers never block: each request reports through its callback, from the network
//! thread. Dropping [`Http`] stops the thread and abandons requests in flight.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// How long connecting may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a download may go without receiving anything.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest package accepted (as the release tool's artifact limit).
const MAX_PACKAGE: u64 = 2 * 1024 * 1024 * 1024;
/// The largest small file (feed, changelog, image) accepted.
const MAX_SMALL: usize = 32 * 1024 * 1024;

/// A download's progress.
#[derive(Debug)]
pub enum Progress {
    /// The response arrived; the size if the server said.
    Started(Option<u64>),
    Received(u64),
}

type Done<T> = Box<dyn FnOnce(Result<T, String>) + Send>;

enum Request {
    Get {
        url: String,
        timeout: Duration,
        done: Done<Vec<u8>>,
    },
    Download {
        url: String,
        file: PathBuf,
        progress: Box<dyn FnMut(Progress) + Send>,
        done: Done<()>,
    },
}

pub struct Http {
    tx: tokio::sync::mpsc::UnboundedSender<Request>,
}

impl Http {
    /// Starts the network thread.
    pub fn new() -> Result<Self, String> {
        let tls = canopy_net::tls::client_config().map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let client = {
            let _enter = runtime.enter();
            reqwest::Client::builder()
                .use_preconfigured_tls((*tls).clone())
                .https_only(true)
                .connect_timeout(CONNECT_TIMEOUT)
                .user_agent(concat!("UnderTheCanopy/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| e.to_string())?
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Request>();
        std::thread::Builder::new()
            .name("canopy-update".into())
            .spawn(move || {
                runtime.block_on(async move {
                    while let Some(request) = rx.recv().await {
                        tokio::spawn(serve(client.clone(), request));
                    }
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { tx })
    }

    /// `GET url` within `timeout`; `done` gets the body.
    pub fn get(
        &self,
        url: &str,
        timeout: Duration,
        done: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static,
    ) {
        let request = Request::Get {
            url: url.to_owned(),
            timeout,
            done: Box::new(done),
        };
        if let Err(tokio::sync::mpsc::error::SendError(Request::Get { done, .. })) =
            self.tx.send(request)
        {
            done(Err("the network thread has stopped".into()));
        }
    }

    /// Downloads `url` into `file` (replaced), reporting progress.
    pub fn download(
        &self,
        url: &str,
        file: PathBuf,
        progress: impl FnMut(Progress) + Send + 'static,
        done: impl FnOnce(Result<(), String>) + Send + 'static,
    ) {
        let request = Request::Download {
            url: url.to_owned(),
            file,
            progress: Box::new(progress),
            done: Box::new(done),
        };
        if let Err(tokio::sync::mpsc::error::SendError(Request::Download { done, .. })) =
            self.tx.send(request)
        {
            done(Err("the network thread has stopped".into()));
        }
    }
}

async fn serve(client: reqwest::Client, request: Request) {
    match request {
        Request::Get { url, timeout, done } => {
            let result = tokio::time::timeout(timeout, get(&client, &url))
                .await
                .unwrap_or_else(|_| Err(format!("{url}: timed out")));
            done(result);
        }
        Request::Download {
            url,
            file,
            mut progress,
            done,
        } => {
            let result = download(&client, &url, &file, &mut progress).await;
            if result.is_err() {
                let _ = std::fs::remove_file(&file);
            }
            done(result);
        }
    }
}

async fn get(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let e = |e: reqwest::Error| format!("{url}: {e}");
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(e)?
        .error_for_status()
        .map_err(e)?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(e)? {
        if body.len() + chunk.len() > MAX_SMALL {
            return Err(format!("{url}: response too large"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &std::path::Path,
    progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<(), String> {
    let e = |e: reqwest::Error| format!("{url}: {e}");
    let mut response = tokio::time::timeout(STALL_TIMEOUT, client.get(url).send())
        .await
        .map_err(|_| format!("{url}: timed out"))?
        .map_err(e)?
        .error_for_status()
        .map_err(e)?;
    let total = response.content_length();
    if total.is_some_and(|t| t > MAX_PACKAGE) {
        return Err(format!("{url}: package too large"));
    }
    progress(Progress::Started(total));
    let io = |err: std::io::Error| format!("{}: {err}", path.display());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let mut file = std::fs::File::create(path).map_err(io)?;
    let mut received = 0u64;
    loop {
        let chunk = tokio::time::timeout(STALL_TIMEOUT, response.chunk())
            .await
            .map_err(|_| format!("{url}: stalled"))?
            .map_err(e)?;
        let Some(chunk) = chunk else {
            break;
        };
        received += chunk.len() as u64;
        if received > MAX_PACKAGE {
            return Err(format!("{url}: package too large"));
        }
        file.write_all(&chunk).map_err(io)?;
        progress(Progress::Received(chunk.len() as u64));
    }
    if total.is_some_and(|t| t != received) {
        return Err(format!("{url}: incomplete ({received} of {total:?} bytes)"));
    }
    file.sync_all().map_err(io)?;
    Ok(())
}
