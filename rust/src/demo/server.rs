use super::{Context, FIXTURES, Report, Run, RunRequest, capacity};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde_json::json;
use std::{collections::VecDeque, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, Semaphore};

struct App {
    context: Context,
    runs: Mutex<VecDeque<Arc<Run>>>,
    workers: Arc<Semaphore>,
}

fn failure(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({"error":message.into()}))).into_response()
}

fn asset(content_type: &'static str, bytes: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response()
}

fn router(state: Arc<App>) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../../demo-ui/public/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_bytes!("../../demo-ui/public/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                asset(
                    "text/css; charset=utf-8",
                    include_bytes!("../../demo-ui/public/style.css"),
                )
            }),
        )
        .route(
            "/vendor/sigplot.js",
            get(|| async {
                asset(
                    "text/javascript; charset=utf-8",
                    include_bytes!("../../demo-ui/public/vendor/sigplot-3.1.7.js"),
                )
            }),
        )
        .route(
            "/vendor/sigplot-3.1.7.js.LEGAL.txt",
            get(|| async {
                asset(
                    "text/plain; charset=utf-8",
                    include_bytes!("../../demo-ui/public/vendor/sigplot-3.1.7.js.LEGAL.txt"),
                )
            }),
        )
        .route(
            "/vendor/LICENSE.sigplot",
            get(|| async {
                asset(
                    "text/plain; charset=utf-8",
                    include_bytes!("../../demo-ui/public/vendor/LICENSE.sigplot"),
                )
            }),
        )
        .route(
            "/vendor/NOTICE.sigplot",
            get(|| async {
                asset(
                    "text/plain; charset=utf-8",
                    include_bytes!("../../demo-ui/public/vendor/NOTICE.sigplot"),
                )
            }),
        )
        .route("/api/config", get(config))
        .route("/api/run", post(execute))
        .route("/api/stream", post(stream))
        .route("/api/runs/{id}/audio.wav", get(audio))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(state)
}

async fn config(State(app): State<Arc<App>>) -> Json<serde_json::Value> {
    Json(json!({
        "sigplot_version":"3.1.7", "sample_rate_hz":48000,
        "levels":(1..=4).map(|level| {
            let (bytes,frames)=capacity(level).expect("known level");
            json!({"level":level,"frame_bytes":bytes,"max_frames":frames,"max_bytes":bytes*frames,"available":level!=4||app.context.profile.is_some()})
        }).collect::<Vec<_>>(),
        "fixtures":FIXTURES.iter().map(|fixture|json!({"id":fixture.id,"label":fixture.label,"level":fixture.level,
            "available":app.context.fixtures.join(format!("{}.wav",fixture.id)).is_file()})).collect::<Vec<_>>(),
        "evidence":"Local codec loopback and recorded-audio replay; no RF or live VARA session",
    }))
}

async fn execute(State(app): State<Arc<App>>, Json(request): Json<RunRequest>) -> Response {
    let Ok(permit) = app.workers.clone().try_acquire_owned() else {
        return failure(
            StatusCode::TOO_MANY_REQUESTS,
            "DSP workers are busy; try again shortly",
        );
    };
    let state = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        super::run(request, &state.context)
    })
    .await;
    match result {
        Ok(Ok(run)) => {
            let run = Arc::new(run);
            let report: Report = run.report.clone();
            let mut runs = app.runs.lock().await;
            while runs.len() >= 4 {
                runs.pop_front();
            }
            runs.push_back(run);
            Json(report).into_response()
        }
        Ok(Err(message)) => failure(StatusCode::BAD_REQUEST, message),
        Err(_) => failure(StatusCode::INTERNAL_SERVER_ERROR, "DSP worker failed"),
    }
}

// Bounded NDJSON with backpressure. Dropping the response cancels the worker.
async fn stream(State(app): State<Arc<App>>, Json(request): Json<RunRequest>) -> Response {
    use tokio_stream::{StreamExt, wrappers::ReceiverStream};
    let Ok(permit) = app.workers.clone().try_acquire_owned() else {
        return failure(StatusCode::TOO_MANY_REQUESTS, "DSP workers are busy");
    };
    let (sender, receiver) = tokio::sync::mpsc::channel::<String>(2);
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let started = std::time::Instant::now();
        let result = super::run_streaming(request, &app.context, true, |progress| {
            let due =
                std::time::Duration::from_secs_f64(progress.received_samples as f64 / 48000.0);
            if let Some(delay) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(delay);
            }
            sender
                .blocking_send(format!("{}\n", json!({"type":"progress", "data":progress})))
                .map_err(|_| "stream cancelled".to_owned())
        });
        let message = match result {
            Ok(run) => {
                let report = run.report.clone();
                runtime.block_on(async {
                    let mut runs = app.runs.lock().await;
                    while runs.len() >= 4 {
                        runs.pop_front();
                    }
                    runs.push_back(Arc::new(run));
                });
                json!({"type":"complete","data":report})
            }
            Err(error) => json!({"type":"error","error":error}),
        };
        let _ = sender.blocking_send(format!("{message}\n"));
    });
    let body =
        Body::from_stream(ReceiverStream::new(receiver).map(Ok::<_, std::convert::Infallible>));
    (
        [
            (header::CONTENT_TYPE, "application/x-ndjson"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

async fn audio(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    let run = app
        .runs
        .lock()
        .await
        .iter()
        .find(|run| run.report.id == id)
        .cloned();
    let Some(run) = run else {
        return failure(
            StatusCode::NOT_FOUND,
            "this run has expired; run the demo again",
        );
    };
    match tokio::task::spawn_blocking(move || super::wav(&run.audio)).await {
        Ok(Ok(bytes)) => {
            let mut headers = HeaderMap::new();
            headers.insert(header::CONTENT_TYPE, "audio/wav".parse().unwrap());
            headers.insert(
                header::CONTENT_DISPOSITION,
                "inline; filename=linkwise-received.wav".parse().unwrap(),
            );
            headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            (headers, Body::from(bytes)).into_response()
        }
        _ => failure(StatusCode::INTERNAL_SERVER_ERROR, "WAV encoding failed"),
    }
}

pub async fn serve(address: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    if !address.ip().is_loopback() {
        return Err("the modem demo binds to a loopback address only".into());
    }
    let context = super::context().map_err(std::io::Error::other)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let address = listener.local_addr()?;
    println!(
        "Linkwise modem lab · Rust DSP + SigPlot 3.1.7\nLocal: http://{address}\nCodec loopback / recorded replay; no radio or live VARA connection.\nPress Ctrl+C to stop."
    );
    let state = Arc::new(App {
        context,
        runs: Mutex::new(VecDeque::new()),
        workers: Arc::new(Semaphore::new(2)),
    });
    axum::serve(listener, router(state)).await?;
    Ok(())
}
