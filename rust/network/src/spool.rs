//! A station owns its database. A positive hop reply follows a committed transaction.
use anyhow::{Context, Result, bail, ensure};
use bp7::{
    Bundle, CreationTimestamp, EndpointID, HOP_COUNT_BLOCK, crc::CRC_32, flags::BundleControlFlags,
};
use redb::{Database, ReadableTable, TableDefinition, WriteTransaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

const CONFIG: TableDefinition<&str, &str> = TableDefinition::new("station_v1");
const ROWS: TableDefinition<&str, &[u8]> = TableDefinition::new("objects_v1");
const WIRE: TableDefinition<&str, &[u8]> = TableDefinition::new("wire_v1");
pub const LIMIT: usize = 64 * 1024;
const MAX_OBJECTS: u64 = 4096;
const MAX_LIFETIME: u64 = 7 * 86400 * 1000;

pub fn endpoint(name: &str) -> Result<EndpointID> {
    ensure!(
        !name.is_empty()
            && name.len() <= 32
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "invalid node name"
    );
    Ok(EndpointID::with_dtn(&format!("{name}/inbox"))?)
}
fn node(eid: &EndpointID) -> Result<String> {
    let text = eid.to_string();
    let name = text
        .strip_prefix("dtn://")
        .and_then(|s| s.strip_suffix("/inbox"))
        .context("unsupported endpoint")?;
    ensure!(endpoint(name)? == *eid, "noncanonical endpoint");
    Ok(name.to_owned())
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Row {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub hash: String,
    pub expires: u64,
    pub state: String,
    pub confirmed: bool,
    pub receipt: bool,
    pub ingress: String,
}

pub struct Station {
    db: Database,
    name: String,
}
impl Station {
    pub fn open(path: &Path, name: &str) -> Result<Self> {
        endpoint(name)?;
        let db = Database::create(path)?;
        let tx = db.begin_write()?;
        {
            let mut config = tx.open_table(CONFIG)?;
            let old = config.get("name")?.map(|x| x.value().to_owned());
            ensure!(
                old.as_deref().is_none_or(|n| n == name),
                "station identity mismatch"
            );
            config.insert("name", name)?;
            tx.open_table(ROWS)?;
            tx.open_table(WIRE)?;
        }
        tx.commit()?;
        Ok(Self {
            db,
            name: name.into(),
        })
    }
    fn make(
        &self,
        tx: &WriteTransaction,
        dest: &str,
        body: Vec<u8>,
        now: u64,
        ttl: u64,
    ) -> Result<Bundle> {
        ensure!(
            now > 0 && ttl > 0 && ttl <= MAX_LIFETIME,
            "invalid timestamp or lifetime"
        );
        let mut cfg = tx.open_table(CONFIG)?;
        let seq = cfg
            .get("sequence")?
            .map(|s| s.value().parse::<u64>())
            .transpose()?
            .unwrap_or(0)
            .checked_add(1)
            .context("sequence exhausted")?;
        cfg.insert("sequence", seq.to_string().as_str())?;
        let mut b =
            bp7::bundle::new_std_payload_bundle(endpoint(&self.name)?, endpoint(dest)?, body);
        b.primary.creation_timestamp = CreationTimestamp::with_time_and_seq(now, seq);
        b.primary.lifetime = Duration::from_millis(ttl);
        b.primary.bundle_control_flags = BundleControlFlags::BUNDLE_MUST_NOT_FRAGMENTED.bits();
        b.set_crc(CRC_32);
        Ok(b)
    }
    // Refuse to age bundles backwards after restart with a rolled-back clock.
    fn check_clock(tx: &WriteTransaction, now: u64) -> Result<()> {
        let mut tab = tx.open_table(CONFIG)?;
        let previous = tab
            .get("clock")?
            .map(|v| v.value().parse::<u64>())
            .transpose()?
            .unwrap_or(0);
        ensure!(now >= previous, "station clock regressed");
        tab.insert("clock", now.to_string().as_str())?;
        Ok(())
    }
    pub fn submit(&self, dest: &str, data: &[u8], now: u64, ttl: u64) -> Result<String> {
        ensure!(
            dest != self.name && data.len() <= LIMIT - 1024,
            "invalid destination or payload size"
        );
        let tx = self.db.begin_write()?;
        Self::check_clock(&tx, now)?;
        let mut body = vec![0];
        body.extend_from_slice(data);
        let mut b = self.make(&tx, dest, body, now, ttl)?;
        let id = b.id();
        self.record(&tx, &mut b, now, false)?;
        tx.commit()?;
        Ok(id)
    }
    fn record(
        &self,
        tx: &WriteTransaction,
        b: &mut Bundle,
        now: u64,
        incoming: bool,
    ) -> Result<()> {
        let source = node(&b.primary.source)?;
        let destination = node(&b.primary.destination)?;
        let ttl = u64::try_from(b.primary.lifetime.as_millis())?;
        let created = b.primary.creation_timestamp.dtntime();
        let expires = created.checked_add(ttl).context("invalid expiry")?;
        ensure!(
            ttl <= MAX_LIFETIME && created > 0 && created <= now && now < expires,
            "outside lifetime"
        );
        let ingress = digest(&b.to_cbor());
        let body = b.payload().context("missing payload")?;
        let (&kind, data) = body.split_first().context("empty application record")?;
        ensure!(kind <= 1, "unknown application type");
        let id = b.id();
        let hash = digest(data);
        let mut rows = tx.open_table(ROWS)?;
        if let Some(previous) = rows.get(id.as_str())? {
            let row: Row = serde_json::from_slice(previous.value())?;
            ensure!(
                row.hash == hash
                    && row.expires == expires
                    && row.destination == destination
                    && row.receipt == (kind == 1),
                "conflicting bundle ID"
            );
            ensure!(
                destination == self.name || row.ingress == ingress,
                "forwarding loop or changed transfer"
            );
            return Ok(());
        }
        use redb::ReadableTableMetadata;
        ensure!(rows.len()? < MAX_OBJECTS, "station quota reached");
        ensure!(
            !incoming || source != self.name,
            "bundle returned to origin"
        );
        let arrived = destination == self.name;
        let row = Row {
            id: id.clone(),
            source: source.clone(),
            destination,
            hash: hash.clone(),
            expires,
            state: if arrived { "delivered" } else { "queued" }.into(),
            confirmed: false,
            receipt: kind == 1,
            ingress,
        };
        if arrived && kind == 1 {
            let (original, expected): (String, String) = serde_json::from_slice(data)?;
            let mut old: Row = serde_json::from_slice(
                rows.get(original.as_str())?
                    .context("unknown receipt target")?
                    .value(),
            )?;
            ensure!(
                !old.receipt
                    && old.source == self.name
                    && old.destination == source
                    && old.hash == expected,
                "receipt mismatch"
            );
            old.confirmed = true;
            rows.insert(original.as_str(), serde_json::to_vec(&old)?.as_slice())?;
        }
        rows.insert(id.as_str(), serde_json::to_vec(&row)?.as_slice())?;
        drop(rows);
        if arrived && kind == 0 {
            let mut ack = vec![1];
            ack.extend(serde_json::to_vec(&(id.as_str(), hash))?);
            let mut receipt = self.make(tx, &source, ack, now, MAX_LIFETIME)?;
            self.record(tx, &mut receipt, now, false)?;
        }
        tx.open_table(WIRE)?
            .insert(id.as_str(), b.to_cbor().as_slice())?;
        Ok(())
    }
    pub fn receive(&self, wire: &[u8], now: u64) -> Result<String> {
        ensure!(wire.len() <= LIMIT, "oversize bundle");
        let mut b = Bundle::try_from(wire)?;
        ensure!(b.validate().is_ok() && b.crc_valid(), "invalid BP bundle");

        ensure!(
            b.primary.crc.to_code() == CRC_32
                && b.canonicals.iter().all(|v| v.crc.to_code() == CRC_32),
            "CRC32C required"
        );
        ensure!(
            b.primary.bundle_control_flags == BundleControlFlags::BUNDLE_MUST_NOT_FRAGMENTED.bits()
                && b.canonicals.len() == 2,
            "unsupported BP profile"
        );
        let (limit, count) = b
            .extension_block_by_type(HOP_COUNT_BLOCK)
            .and_then(|v| v.hop_count_get())
            .context("missing hop limit")?;
        ensure!(
            limit == 32 && count > 0 && count <= limit,
            "invalid hop count"
        );
        let tx = self.db.begin_write()?;
        Self::check_clock(&tx, now)?;
        self.record(&tx, &mut b, now, true)?;
        tx.commit()?;
        Ok(b.id())
    }
    pub fn list(&self) -> Result<Vec<Row>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(ROWS)?;
        table
            .iter()?
            .map(|item| Ok(serde_json::from_slice(item?.1.value())?))
            .collect()
    }
    pub fn bytes(&self, id: &str) -> Result<Vec<u8>> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(WIRE)?;
        Ok(table
            .get(id)?
            .context("no retained wire object")?
            .value()
            .to_vec())
    }
    pub fn outgoing(&self, id: &str, now: u64) -> Result<Vec<u8>> {
        let tx = self.db.begin_write()?;
        Self::check_clock(&tx, now)?;
        tx.commit()?;
        let row = self
            .list()?
            .into_iter()
            .find(|r| r.id == id)
            .context("unknown object")?;
        ensure!(
            row.state == "queued" && now < row.expires,
            "object unavailable"
        );
        let mut b = Bundle::try_from(self.bytes(id)?)?;
        let h = b
            .extension_block_by_type_mut(HOP_COUNT_BLOCK)
            .context("missing hop count")?;
        ensure!(
            h.hop_count_increase() && !h.hop_count_exceeded(),
            "hop limit"
        );
        Ok(b.to_cbor())
    }
    pub fn finish(&self, id: &str) -> Result<()> {
        let tx = self.db.begin_write()?;
        {
            let mut rows = tx.open_table(ROWS)?;
            let mut row: Row =
                serde_json::from_slice(rows.get(id)?.context("unknown object")?.value())?;
            ensure!(row.state == "queued", "object already retired");
            row.state = "forwarded".into();
            rows.insert(id, serde_json::to_vec(&row)?.as_slice())?;
        }
        tx.open_table(WIRE)?.remove(id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn read_payload(&self, id: &str) -> Result<Vec<u8>> {
        let row = self
            .list()?
            .into_iter()
            .find(|r| r.id == id)
            .context("unknown object")?;
        if row.state != "delivered" || row.receipt {
            bail!("not a delivered data object")
        }
        let b = Bundle::try_from(self.bytes(id)?)?;
        Ok(b.payload().context("missing payload")?[1..].to_vec())
    }
    pub fn prune(&self, now: u64) -> Result<usize> {
        let mut expired = Vec::new();
        let tx = self.db.begin_write()?;
        Self::check_clock(&tx, now)?;
        {
            let mut rows = tx.open_table(ROWS)?;
            for item in rows.iter()? {
                let (_, v) = item?;
                let row: Row = serde_json::from_slice(v.value())?;
                if row.expires.saturating_add(MAX_LIFETIME) <= now {
                    expired.push(row.id);
                }
            }
            let mut wire = tx.open_table(WIRE)?;
            for id in &expired {
                rows.remove(id.as_str())?;
                wire.remove(id.as_str())?;
            }
        }
        tx.commit()?;
        Ok(expired.len())
    }
}

const PART: TableDefinition<&str, &[u8]> = TableDefinition::new("incomplete_v1");
impl Station {
    // One bounded staging slot per station. A different transfer may reclaim it.
    pub fn partial(&self, hash: &str) -> Result<Vec<u8>> {
        let tx = self.db.begin_write()?;
        let result = {
            let mut tab = tx.open_table(PART)?;
            let matches = tab
                .get("hash")?
                .is_some_and(|v| v.value() == hash.as_bytes());
            if matches {
                tab.get("bytes")?
                    .map(|v| v.value().to_vec())
                    .unwrap_or_default()
            } else {
                tab.remove("bytes")?;
                tab.insert("hash", hash.as_bytes())?;
                Vec::new()
            }
        };
        tx.commit()?;
        Ok(result)
    }
    pub fn checkpoint(&self, hash: &str, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= LIMIT, "partial transfer limit");
        let tx = self.db.begin_write()?;
        {
            let mut tab = tx.open_table(PART)?;
            ensure!(
                tab.get("hash")?
                    .is_some_and(|v| v.value() == hash.as_bytes()),
                "transfer displaced"
            );
            tab.insert("bytes", bytes)?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lost_ack_restart_and_receipt() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let a = Station::open(&dir.path().join("a"), "A")?;
        let path = dir.path().join("b");
        let id = a.submit("B", &[0, 255], 100, 20)?;
        let wire = a.outgoing(&id, 101)?;
        {
            let b = Station::open(&path, "B")?;
            b.receive(&wire, 102)?;
        }
        let b = Station::open(&path, "B")?;
        b.receive(&wire, 103)?;
        assert_eq!(b.list()?.len(), 2);
        assert_eq!(b.read_payload(&id)?, [0, 255]);
        let receipt = b.list()?.into_iter().find(|r| r.receipt).unwrap();
        a.finish(&id)?;
        a.receive(&b.outgoing(&receipt.id, 104)?, 105)?;
        assert!(a.list()?[0].confirmed);
        assert!(b.receive(&wire, 101).is_err());
        assert!(b.receive(&wire, 121).is_err());
        assert!(b.receive(&wire[..8], 122).is_err());
        Ok(())
    }
    #[test]
    fn partial_survives_reopen_and_can_be_displaced() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("b");
        let b = Station::open(&path, "B")?;
        b.partial("one")?;
        b.checkpoint("one", &[7; 1024])?;
        drop(b);
        let b = Station::open(&path, "B")?;
        assert_eq!(b.partial("one")?, vec![7; 1024]);
        assert!(b.partial("two")?.is_empty());
        assert!(b.checkpoint("one", &[4]).is_err());
        assert!(b.checkpoint("two", &vec![0; LIMIT + 1]).is_err());
        Ok(())
    }
}
