//! One transfer per reliable duplex session. Fixed-size handshake, bounded
//! writes and durable offsets support resumption after stream interruption.
use crate::spool::{LIMIT, Station, digest};
use anyhow::{Result, ensure};
use std::io::{Read, Write};
const CHUNK: usize = 1024;
const MAGIC: &[u8; 4] = b"LWN1";
fn number(s: &mut impl Read) -> Result<usize> {
    let mut b = [0; 4];
    s.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b) as usize)
}
fn put(s: &mut impl Write, n: usize) -> Result<()> {
    s.write_all(&u32::try_from(n)?.to_be_bytes())?;
    s.flush()?;
    Ok(())
}
pub fn send(st: &Station, id: &str, s: &mut (impl Read + Write), now: u64) -> Result<()> {
    let bytes = st.outgoing(id, now)?;
    let hash = digest(&bytes);
    s.write_all(MAGIC)?;
    s.write_all(hash.as_bytes())?;
    put(s, bytes.len())?;
    let mut offset = number(s)?;
    ensure!(offset <= bytes.len(), "invalid receiver offset");
    while offset < bytes.len() {
        let end = (offset + CHUNK).min(bytes.len());
        s.write_all(&bytes[offset..end])?;
        s.flush()?;
        ensure!(number(s)? == end, "invalid progress");
        offset = end;
    }
    let mut receipt = [0; 68];
    s.read_exact(&mut receipt)?;
    ensure!(
        &receipt[..4] == b"SYNC" && &receipt[4..] == hash.as_bytes(),
        "invalid durable acceptance"
    );
    st.finish(id)
}
pub fn receive(
    st: &Station,
    s: &mut (impl Read + Write),
    clock: impl Fn() -> u64,
) -> Result<String> {
    let mut magic = [0; 4];
    s.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "wrong transfer profile");
    let mut h = [0; 64];
    s.read_exact(&mut h)?;
    ensure!(h.iter().all(u8::is_ascii_hexdigit), "invalid digest");
    let hash = std::str::from_utf8(&h)?;
    let n = number(s)?;
    ensure!((1..=LIMIT).contains(&n), "size outside profile");
    let mut data = st.partial(hash)?;
    ensure!(data.len() <= n, "inconsistent transfer length");
    put(s, data.len())?;
    while data.len() < n {
        let start = data.len();
        data.resize((start + CHUNK).min(n), 0);
        s.read_exact(&mut data[start..])?;
        st.checkpoint(hash, &data)?;
        put(s, data.len())?;
    }
    if digest(&data) != hash {
        st.checkpoint(hash, &[])?;
        anyhow::bail!("wire digest mismatch")
    }
    let id = st.receive(&data, clock())?;
    s.write_all(b"SYNC")?;
    s.write_all(&h)?;
    s.flush()?;
    Ok(id)
}
