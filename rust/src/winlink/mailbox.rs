//! Directory-backed P2P mailbox. The application owns a private root directory
//! and should serialize forwarding sessions that share it.

use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Result, Write},
    path::{Path, PathBuf},
};

use super::{
    Message, invalid,
    session::{Delivery, Mailbox},
    valid_mid,
};

pub struct DirectoryMailbox {
    root: PathBuf,
    max_message_bytes: usize,
}

impl DirectoryMailbox {
    pub fn open(root: impl Into<PathBuf>, max_message_bytes: usize) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        for name in ["inbox", "outbox", "finished"] {
            fs::create_dir_all(root.join(name))?;
        }
        sync_directory(&root)?;
        if let Some(parent) = root.parent().filter(|path| !path.as_os_str().is_empty()) {
            sync_directory(parent)?;
        }
        Ok(Self {
            root,
            max_message_bytes,
        })
    }

    /// Queue an envelope. MIDs are unique for the lifetime of this mailbox.
    pub fn enqueue(&self, envelope: &[u8]) -> Result<()> {
        let message = Message::decode(envelope, self.max_message_bytes)?;
        let name = message_filename(message.id)?;
        if self.root.join("finished").join(&name).try_exists()? {
            return Err(invalid("B2F message ID was already finalized"));
        }
        publish(&self.root.join("outbox").join(name), envelope)
    }

    pub fn incoming(&self, id: &str) -> Result<Vec<u8>> {
        self.read(&self.root.join("inbox").join(message_filename(id)?))
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let mut wire = Vec::new();
        File::open(path)?
            .take((self.max_message_bytes as u64).saturating_add(1))
            .read_to_end(&mut wire)?;
        if wire.len() > self.max_message_bytes {
            return Err(invalid("mailbox envelope exceeds configured limit"));
        }
        Ok(wire)
    }
}

impl Mailbox for DirectoryMailbox {
    fn outgoing(&mut self, peer: &str, excluded: &[String]) -> Result<Vec<Vec<u8>>> {
        let outbox = self.root.join("outbox");
        let finished = self.root.join("finished");
        let mut paths = fs::read_dir(outbox)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>>>()?;
        paths.sort_unstable();
        let mut messages = Vec::new();
        for path in paths {
            if path.extension().is_none_or(|ext| ext != "b2f") {
                continue;
            }
            if finished
                .join(path.file_name().expect("directory entry name"))
                .try_exists()?
            {
                continue;
            }
            let wire = self.read(&path)?;
            let message = Message::decode(&wire, self.max_message_bytes)?;
            if message_filename(message.id)?
                != path
                    .file_name()
                    .expect("directory entry name")
                    .to_string_lossy()
            {
                return Err(invalid("outbox filename differs from message ID"));
            }
            if excluded.iter().any(|id| id == message.id) {
                continue;
            }
            if message
                .to
                .iter()
                .chain(&message.cc)
                .all(|call| call.eq_ignore_ascii_case(peer))
            {
                messages.push(wire);
                if messages.len() == super::session::BATCH {
                    break;
                }
            }
        }
        Ok(messages)
    }

    fn contains(&mut self, id: &str) -> Result<bool> {
        let inbox = self.root.join("inbox");
        if !inbox.join(message_filename(id)?).try_exists()? {
            return Ok(false);
        }
        // A peer may have lost the acknowledgment, or the previous directory
        // sync may have failed after the file was published.
        sync_directory(&inbox)?;
        Ok(true)
    }

    fn store(&mut self, id: &str, envelope: &[u8]) -> Result<()> {
        let message = Message::decode(envelope, self.max_message_bytes)?;
        if message.id != id {
            return Err(invalid("mailbox message ID differs from envelope"));
        }
        let path = self.root.join("inbox").join(message_filename(id)?);
        match publish(&path, envelope) {
            Err(error)
                if error.kind() == ErrorKind::AlreadyExists && self.read(&path)? == envelope =>
            {
                sync_directory(path.parent().expect("mailbox directory"))
            }
            result => result,
        }
    }

    fn finish(&mut self, id: &str, delivery: Delivery) -> Result<()> {
        let name = message_filename(id)?;
        let finished = self.root.join("finished").join(&name);
        let marker = match delivery {
            Delivery::Confirmed => b"confirmed\n".as_slice(),
            Delivery::Rejected => b"rejected\n".as_slice(),
        };
        match publish(&finished, marker) {
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                sync_directory(finished.parent().expect("mailbox directory"))?;
            }
            result => result?,
        }
        let outbox = self.root.join("outbox");
        match fs::remove_file(outbox.join(name)) {
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            result => result?,
        }
        sync_directory(&outbox)
    }
}

fn message_filename(id: &str) -> Result<String> {
    use std::fmt::Write as _;
    if !valid_mid(id) {
        return Err(invalid("invalid mailbox message ID"));
    }
    let mut name = String::with_capacity(id.len() * 2 + 4);
    for byte in id.bytes() {
        write!(name, "{byte:02x}").expect("writing to String");
    }
    name.push_str(".b2f");
    Ok(name)
}

pub(super) fn publish(path: &Path, data: &[u8]) -> Result<()> {
    let directory = path.parent().expect("mailbox directory");
    let temporary = directory.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        // Publish the complete file without replacing a competing writer.
        #[cfg(not(target_os = "android"))]
        fs::hard_link(&temporary, path)?;
        // Android SELinux can deny hard links even in writable directories.
        #[cfg(target_os = "android")]
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            &temporary,
            rustix::fs::CWD,
            path,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        sync_directory(directory)
    })();
    let _ = fs::remove_file(temporary);
    result
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}
