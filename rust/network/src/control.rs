//! Owner-local queue access for a running bench station.
use crate::{
    bounded,
    spool::{LIMIT, Row, Station},
    ttl_millis,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const REQUEST_LIMIT: usize = 256 * 1024;
const REPLY_LIMIT: usize = 4 * 1024 * 1024;
const IO_BUDGET: Duration = Duration::from_secs(3);

#[derive(Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Submit {
        request_id: String,
        destination: String,
        payload: Vec<u8>,
        ttl_seconds: u64,
    },
    List,
    Read {
        id: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum Reply {
    Submitted { id: String },
    Listed { rows: Vec<Row> },
    Payload { bytes: Vec<u8> },
    Error { message: String },
}

fn before_deadline<T>(
    deadline: Instant,
    mut operation: impl FnMut() -> io::Result<T>,
) -> Result<T> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|time| !time.is_zero())
            .context("queue request timed out")?;
        match operation() {
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(remaining.min(Duration::from_millis(5)));
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => return Ok(result?),
        }
    }
}

fn read_exact(stream: &mut UnixStream, mut bytes: &mut [u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        let count =
            before_deadline(deadline, || stream.read(bytes)).context("read queue record")?;
        ensure!(count != 0, "incomplete queue record");
        bytes = &mut bytes[count..];
    }
    Ok(())
}

fn read_frame<T: DeserializeOwned>(
    stream: &mut UnixStream,
    limit: usize,
    deadline: Instant,
) -> Result<T> {
    let mut header = [0; 4];
    read_exact(stream, &mut header, deadline)?;
    let len = u32::from_be_bytes(header) as usize;
    ensure!(
        (1..=limit).contains(&len),
        "queue record size outside profile"
    );
    let mut body = vec![0; len];
    read_exact(stream, &mut body, deadline)?;
    Ok(serde_json::from_slice(&body)?)
}

fn write_frame(
    stream: &mut UnixStream,
    body: &impl Serialize,
    limit: usize,
    deadline: Instant,
) -> Result<()> {
    let mut frame = vec![0; 4];
    serde_json::to_writer(&mut frame, body)?;
    let len = frame.len() - 4;
    ensure!(len <= limit, "queue record size outside profile");
    frame[..4].copy_from_slice(&u32::try_from(len)?.to_be_bytes());
    let mut bytes = frame.as_slice();
    while !bytes.is_empty() {
        let count =
            before_deadline(deadline, || stream.write(bytes)).context("write queue record")?;
        ensure!(count != 0, "could not write queue record");
        bytes = &bytes[count..];
    }
    Ok(())
}

struct Listener {
    socket: UnixListener,
    path: PathBuf,
    identity: (u64, u64),
}

impl Listener {
    fn bind(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let directory = fs::metadata(parent)
            .with_context(|| format!("read queue socket directory {}", parent.display()))?;
        ensure!(
            directory.is_dir() && directory.permissions().mode() & 0o077 == 0,
            "queue socket directory must be private to the station owner"
        );
        let socket = UnixListener::bind(path)
            .with_context(|| format!("bind queue socket {}", path.display()))?;
        let file = fs::symlink_metadata(path)?;
        let control = Self {
            socket,
            path: path.to_owned(),
            identity: (file.dev(), file.ino()),
        };
        control.socket.set_nonblocking(true)?;
        Ok(control)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Ok(file) = fs::symlink_metadata(&self.path)
            && file.file_type().is_socket()
            && (file.dev(), file.ino()) == self.identity
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn handle(station: &Station, request: Request) -> Result<Reply> {
    match request {
        Request::Submit {
            request_id,
            destination,
            payload,
            ttl_seconds,
        } => Ok(Reply::Submitted {
            id: station.submit(
                &destination,
                &payload,
                bp7::dtn_time_now,
                ttl_millis(ttl_seconds)?,
                Some(&request_id),
            )?,
        }),
        Request::List => Ok(Reply::Listed {
            rows: station.list()?,
        }),
        Request::Read { id } => Ok(Reply::Payload {
            bytes: station.read_payload(&id)?,
        }),
    }
}

fn exchange(station: &Station, mut stream: UnixStream) -> Result<()> {
    stream.set_nonblocking(true)?;
    let deadline = Instant::now() + IO_BUDGET;
    let reply = match read_frame(&mut stream, REQUEST_LIMIT, deadline)
        .and_then(|request| handle(station, request))
    {
        Ok(reply) => reply,
        Err(err) => Reply::Error {
            message: err.to_string(),
        },
    };
    write_frame(&mut stream, &reply, REPLY_LIMIT, deadline)
}

pub fn with_listener(
    path: &Path,
    station: &Station,
    run: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let listener = Listener::bind(path)?;
    thread::scope(|scope| {
        let (shutdown, finished) = mpsc::channel::<()>();
        let worker = scope.spawn(move || -> Result<()> {
            loop {
                if !matches!(finished.try_recv(), Err(TryRecvError::Empty)) {
                    return Ok(());
                }
                match listener.socket.accept() {
                    Ok((stream, _)) => {
                        if let Err(err) = exchange(station, stream) {
                            eprintln!("queue request: {err}");
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                        if !matches!(
                            finished.recv_timeout(Duration::from_millis(10)),
                            Err(RecvTimeoutError::Timeout)
                        ) {
                            return Ok(());
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    Err(err) => return Err(err).context("accept queue request"),
                }
            }
        });
        let result = run();
        drop(shutdown);
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("queue worker panicked"))??;
        result
    })
}

pub fn command(path: &Path, args: &[&str]) -> Result<()> {
    let request = match args {
        ["submit", request_id, destination, file, seconds] => Request::Submit {
            request_id: (*request_id).to_owned(),
            destination: (*destination).to_owned(),
            payload: bounded(file, LIMIT - 1024)?,
            ttl_seconds: seconds.parse()?,
        },
        ["list"] => Request::List,
        ["read", id] => Request::Read {
            id: (*id).to_owned(),
        },
        _ => bail!(crate::USAGE),
    };
    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("connect to queue socket {}", path.display()))?;
    stream.set_nonblocking(true)?;
    let deadline = Instant::now() + IO_BUDGET;
    write_frame(&mut stream, &request, REQUEST_LIMIT, deadline)?;
    match read_frame(&mut stream, REPLY_LIMIT, deadline)? {
        Reply::Submitted { id } => println!("{id}"),
        Reply::Listed { rows } => println!("{}", serde_json::to_string_pretty(&rows)?),
        Reply::Payload { bytes } => io::stdout().write_all(&bytes)?,
        Reply::Error { message } => bail!("{message}"),
    }
    Ok(())
}
