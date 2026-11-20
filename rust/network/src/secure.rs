//! Authenticated records for the private loopback network bench. Both nonces
//! and the direction bind every frame to this session. Payloads are plaintext.
use anyhow::{Context, Result, ensure};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::{self, Read, Write};
use std::net::TcpStream;
type Tag = Hmac<Sha256>;
pub fn key(s: &str) -> Result<[u8; 32]> {
    ensure!(
        s.len() == 64 && s.is_ascii(),
        "peer key must be 32 hex bytes"
    );
    let mut k = [0; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16)?;
    }
    Ok(k)
}
pub struct Stream {
    socket: TcpStream,
    session: Tag,
    side: u8,
    rx: u64,
    tx: u64,
    buf: io::Cursor<Vec<u8>>,
}
impl Stream {
    pub fn new(
        mut socket: TcpStream,
        key: &[u8; 32],
        caller: bool,
        caller_name: &str,
        callee_name: &str,
    ) -> Result<Self> {
        let mut ours = [0; 32];
        getrandom::getrandom(&mut ours)
            .ok()
            .context("random source unavailable")?;
        socket.write_all(&ours)?;
        let mut theirs = [0; 32];
        socket.read_exact(&mut theirs)?;
        let mut session = Tag::new_from_slice(key)?;
        session.update(b"Linkwise network bench v1");
        for n in [caller_name, callee_name] {
            session.update(&[n.len() as u8]);
            session.update(n.as_bytes());
        }
        for n in if caller {
            [ours, theirs]
        } else {
            [theirs, ours]
        } {
            session.update(&n);
        }
        Ok(Self {
            socket,
            session,
            side: caller as u8,
            rx: 0,
            tx: 0,
            buf: io::Cursor::new(vec![]),
        })
    }
    fn tag(&self, side: u8, seq: u64, bytes: &[u8]) -> Tag {
        let mut tag = self.session.clone();
        tag.update(&[side]);
        tag.update(&seq.to_be_bytes());
        tag.update(bytes);
        tag
    }
}
impl Write for Stream {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = b.len().min(8192);
        if n == 0 {
            return Ok(0);
        }
        let tag = self
            .tag(self.side, self.tx, &b[..n])
            .finalize()
            .into_bytes();
        self.socket.write_all(&(n as u16).to_be_bytes())?;
        self.socket.write_all(&b[..n])?;
        self.socket.write_all(&tag)?;
        self.tx += 1;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
impl Read for Stream {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        let n = self.buf.read(b)?;
        if n != 0 {
            return Ok(n);
        }
        let mut h = [0; 2];
        self.socket.read_exact(&mut h)?;
        let n = u16::from_be_bytes(h) as usize;
        if !(1..=8192).contains(&n) {
            return Err(io::Error::other("invalid authenticated record size"));
        }
        let mut bytes = vec![0; n];
        self.socket.read_exact(&mut bytes)?;
        let mut mac = [0; 32];
        self.socket.read_exact(&mut mac)?;
        self.tag(1 - self.side, self.rx, &bytes)
            .verify_slice(&mac)
            .map_err(io::Error::other)?;
        self.rx += 1;
        self.buf = io::Cursor::new(bytes);
        self.buf.read(b)
    }
}
