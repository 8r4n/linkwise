use super::{Error, Result};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;

#[derive(Deserialize, Serialize)]
pub(super) struct Record<T> {
    pub version: u8,
    pub profile_id: String,
    pub at: DateTime<Utc>,
    pub event: T,
}

pub(super) struct Journal {
    dir: PathBuf,
    _lock: File,
    sequence: u64,
    faulted: bool,
}

impl Journal {
    pub fn open<T: serde::de::DeserializeOwned>(
        dir: &Path,
        mut visit: impl FnMut(u64, Record<T>) -> Result<()>,
    ) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("station.lock"))?;
        FileExt::try_lock_exclusive(&lock).map_err(|error| {
            if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                Error::Busy("another station manager owns this journal")
            } else {
                Error::Io(error)
            }
        })?;
        let mut sequence = 0;
        let mut records = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
                continue;
            };
            if stem.len() == 20
                && let Ok(index) = stem.parse::<u64>()
            {
                records.push((index, entry.path()));
            }
        }
        records.sort_unstable_by_key(|(index, _)| *index);
        for (index, path) in records {
            if index != sequence + 1 {
                return Err(Error::Invalid("missing station journal record"));
            }
            visit(index, Self::read(path)?)?;
            sequence = index;
        }
        Ok(Self {
            dir: dir.to_owned(),
            _lock: lock,
            sequence,
            faulted: false,
        })
    }

    pub fn load<T: serde::de::DeserializeOwned>(&self, sequence: u64) -> Result<Record<T>> {
        Self::read(self.dir.join(format!("{sequence:020}.json")))
    }

    fn read<T: serde::de::DeserializeOwned>(path: PathBuf) -> Result<Record<T>> {
        let record: Record<T> = serde_json::from_reader(io::BufReader::new(File::open(path)?))?;
        if record.version != 1 {
            return Err(Error::Invalid("unsupported station journal version"));
        }
        Ok(record)
    }

    pub fn append<T: Serialize>(
        &mut self,
        profile_id: &str,
        at: DateTime<Utc>,
        event: T,
    ) -> Result<u64> {
        if self.faulted {
            return Err(Error::Denied("station journal requires recovery"));
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(Error::Invalid("journal sequence exhausted"))?;
        let record = Record {
            version: 1,
            profile_id: profile_id.into(),
            at,
            event,
        };
        let result = self.publish(sequence, &record);
        match result {
            Ok(()) => self.sequence = sequence,
            Err(_) => self.faulted = true,
        }
        result.map(|()| sequence)
    }

    fn publish<T: Serialize>(&self, sequence: u64, record: &Record<T>) -> Result<()> {
        let file = NamedTempFile::new_in(&self.dir)?;
        serde_json::to_writer(file.as_file(), record)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(self.dir.join(format!("{sequence:020}.json")))
            .map_err(|error| error.error)?;
        #[cfg(unix)]
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }
}
