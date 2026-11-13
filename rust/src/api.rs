//! Bounded CR-delimited host command codec for VARA-style clients.
//!
//! This codec is transport-independent. Parsing a command is not acceptance by
//! a modem: the session owner must check state, authority, configured identity
//! and audio readiness before executing it. Unsupported modes return WRONG.

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    MyCall(Vec<String>),
    Listen(bool),
    Chat(bool),
    IgnoreKissDcd(bool),
    Connect { source: String, destination: String },
    Abort,
    Disconnect,
    Bw500,
    Bw2300,
    Bw2750,
    CompressionOff,
    CompressionOn,
    CompressionText,
    Public(bool),
    Cwid(bool),
    P2pSession,
    WinlinkSession,
    Version,
    ClearTxBuffer,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Wrong;

#[derive(Default)]
pub struct CommandDecoder {
    line: Vec<u8>,
    discarded: bool,
}

impl CommandDecoder {
    /// Several commands can share a chunk; a command can span any number of
    /// chunks. Oversized/non-ASCII lines are discarded through their next CR.
    pub fn push(&mut self, bytes: &[u8], mut report: impl FnMut(Result<Command, Wrong>)) {
        for &byte in bytes {
            if byte == b'\r' {
                report(if self.discarded {
                    Err(Wrong)
                } else {
                    parse(&self.line)
                });
                self.line.clear();
                self.discarded = false;
            } else if !byte.is_ascii_graphic() && byte != b' ' || self.line.len() >= 512 {
                self.discarded = true;
                self.line.clear();
            } else if !self.discarded {
                self.line.push(byte);
            }
        }
    }
}

fn call(value: &str) -> Option<String> {
    let setup = crate::modem::setup::LinkSetup {
        caller: value.into(),
        high_speed: false,
        requested_level: 4,
    };
    let profile = crate::modem::setup::Profile::Bw500;
    let wire = setup.encode(profile)?;
    Some(crate::modem::setup::LinkSetup::decode(profile, &wire)?.caller)
}

fn parse(line: &[u8]) -> Result<Command, Wrong> {
    let line = std::str::from_utf8(line)
        .map_err(|_| Wrong)?
        .to_ascii_uppercase();
    let fields: Vec<_> = line.split_ascii_whitespace().collect();
    Ok(match fields.as_slice() {
        ["MYCALL", calls @ ..] if (1..=5).contains(&calls.len()) => Command::MyCall(
            calls
                .iter()
                .map(|value| {
                    if let Some(base) = value.strip_suffix("-T") {
                        let base = call(base).ok_or(Wrong)?;
                        if base.contains('-') {
                            return Err(Wrong);
                        }
                        Ok(format!("{base}-T"))
                    } else {
                        call(value).ok_or(Wrong)
                    }
                })
                .collect::<Result<_, _>>()?,
        ),
        ["CHAT", "ON"] => Command::Chat(true),
        ["CHAT", "OFF"] => Command::Chat(false),
        ["IGNOREKISSDCD", "ON"] => Command::IgnoreKissDcd(true),
        ["IGNOREKISSDCD", "OFF"] => Command::IgnoreKissDcd(false),
        ["LISTEN", "ON"] => Command::Listen(true),
        ["LISTEN", "OFF"] => Command::Listen(false),
        ["CONNECT", source, destination] => Command::Connect {
            source: call(source).ok_or(Wrong)?,
            destination: call(destination).ok_or(Wrong)?,
        },
        ["ABORT"] => Command::Abort,
        ["DISCONNECT"] => Command::Disconnect,
        ["BW500"] => Command::Bw500,
        ["BW2300"] => Command::Bw2300,
        ["BW2750"] => Command::Bw2750,
        ["COMPRESSION", "OFF"] => Command::CompressionOff,
        ["COMPRESSION", "ON"] => Command::CompressionOn,
        ["COMPRESSION", "TEXT"] => Command::CompressionText,
        ["PUBLIC", "ON"] => Command::Public(true),
        ["PUBLIC", "OFF"] => Command::Public(false),
        ["CWID", "ON"] => Command::Cwid(true),
        ["CWID", "OFF"] => Command::Cwid(false),
        ["P2P", "SESSION"] => Command::P2pSession,
        ["WINLINK", "SESSION"] => Command::WinlinkSession,
        ["VERSION"] => Command::Version,
        ["CLEANTXBUFFER"] => Command::ClearTxBuffer,
        _ => return Err(Wrong),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fragmented_and_coalesced_commands_without_losing_boundaries() {
        let input = b"MYCALL w9ssj\rBW500\rLISTEN ON\rCONNECT W9SSJ W1AW\r";
        for size in [1, 7, input.len()] {
            let mut decoder = CommandDecoder::default();
            let mut commands = Vec::new();
            for block in input.chunks(size) {
                decoder.push(block, |command| commands.push(command));
            }
            assert_eq!(
                commands,
                vec![
                    Ok(Command::MyCall(vec!["W9SSJ".into()])),
                    Ok(Command::Bw500),
                    Ok(Command::Listen(true)),
                    Ok(Command::Connect {
                        source: "W9SSJ".into(),
                        destination: "W1AW".into()
                    })
                ]
            );
        }
    }

    #[test]
    fn malformed_commands_resynchronize_without_executing_suffixes() {
        let mut decoder = CommandDecoder::default();
        let mut commands = Vec::new();
        decoder.push(&[b'X'; 513], |command| commands.push(command));
        decoder.push(
            b"ABORT\rVERSION\r\xff\rMYCALL bad_call\rCOMPRESSION TEXT EXTRA\r",
            |command| commands.push(command),
        );
        assert_eq!(
            commands,
            [
                Err(Wrong),
                Ok(Command::Version),
                Err(Wrong),
                Err(Wrong),
                Err(Wrong)
            ]
        );
    }

    #[test]
    fn parses_observed_winlink_and_pat_setup_commands() {
        let mut commands = Vec::new();
        CommandDecoder::default().push(
            b"PUBLIC ON\rCWID ON\rCOMPRESSION ON\rCOMPRESSION TEXT\rBW2750\rBW2300\rP2P SESSION\rWINLINK SESSION\r",
            |command| commands.push(command),
        );
        assert_eq!(
            commands,
            [
                Ok(Command::Public(true)),
                Ok(Command::Cwid(true)),
                Ok(Command::CompressionOn),
                Ok(Command::CompressionText),
                Ok(Command::Bw2750),
                Ok(Command::Bw2300),
                Ok(Command::P2pSession),
                Ok(Command::WinlinkSession),
            ]
        );
    }
}
