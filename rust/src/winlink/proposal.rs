//! B2F encapsulated-message proposal blocks.

use std::io::Result;

use super::{checksum, invalid, length, valid_mid};

const MAX_PROPOSALS: usize = 5;
const MAX_BLOCK_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalKind {
    Message,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Proposal<'a> {
    pub kind: ProposalKind,
    pub id: &'a str,
    pub message_bytes: usize,
    pub compressed_bytes: usize,
}

impl Proposal<'_> {
    fn validate(&self, max_message_bytes: usize, max_compressed_bytes: usize) -> Result<()> {
        if !valid_mid(self.id)
            || !(1..=max_message_bytes).contains(&self.message_bytes)
            || !(1..=max_compressed_bytes).contains(&self.compressed_bytes)
        {
            return Err(invalid("invalid B2F proposal ID or size"));
        }
        Ok(())
    }
}

/// The peer's disposition for one proposed message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answer {
    Accept { offset: usize },
    Reject,
    Defer,
}

/// Parse one FS reply with exactly one answer per proposal.
pub fn decode_answers(wire: &str, proposals: &[Proposal<'_>]) -> Result<Vec<Answer>> {
    if !(1..=MAX_PROPOSALS).contains(&proposals.len()) || wire.len() > 4 + MAX_PROPOSALS * 7 {
        return Err(invalid("invalid B2F proposal reply size"));
    }
    let mut reply = wire
        .strip_prefix("FS ")
        .and_then(|line| line.strip_suffix('\r'))
        .ok_or_else(|| invalid("invalid B2F proposal reply"))?;
    let mut answers = Vec::with_capacity(proposals.len());
    for proposal in proposals {
        let Some((&code, _)) = reply.as_bytes().split_first() else {
            return Err(invalid("missing B2F proposal answer"));
        };
        if !code.is_ascii() {
            return Err(invalid("invalid B2F proposal answer"));
        }
        reply = &reply[1..];
        answers.push(match code.to_ascii_uppercase() {
            b'+' | b'Y' => Answer::Accept { offset: 0 },
            b'-' | b'N' | b'R' => Answer::Reject,
            b'=' | b'L' | b'H' => Answer::Defer,
            b'!' | b'A' => {
                let end = reply.bytes().take_while(u8::is_ascii_digit).count();
                if end > 6 {
                    return Err(invalid("invalid B2F resume offset"));
                }
                let offset = length(&reply[..end])?;
                if offset > super::transfer::MAX_OFFSET || offset > proposal.compressed_bytes {
                    return Err(invalid("invalid B2F resume offset"));
                }
                reply = &reply[end..];
                Answer::Accept { offset }
            }
            _ => return Err(invalid("invalid B2F proposal answer")),
        });
    }
    if !reply.is_empty() {
        return Err(invalid("extra B2F proposal answers"));
    }
    Ok(answers)
}

/// Parse up to five FC proposals followed by a verified F> line.
/// The optional compatibility field is accepted only with value zero.
pub fn decode(
    wire: &[u8],
    max_message_bytes: usize,
    max_compressed_bytes: usize,
) -> Result<Vec<Proposal<'_>>> {
    if wire.len() > MAX_BLOCK_BYTES {
        return Err(invalid("oversized B2F proposal block"));
    }
    let text = std::str::from_utf8(wire).map_err(|_| invalid("invalid B2F proposal encoding"))?;
    let (lines, checksum_line) = text
        .strip_suffix('\r')
        .and_then(|block| block.rsplit_once('\r'))
        .ok_or_else(|| invalid("missing B2F proposal terminator"))?;
    let supplied = checksum_line
        .strip_prefix("F> ")
        .filter(|s| s.len() == 2 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|s| u8::from_str_radix(s, 16).ok());
    if supplied != Some(checksum(&wire[..lines.len() + 1])) {
        return Err(invalid("invalid B2F proposal checksum"));
    }
    let mut proposals = Vec::new();
    for line in lines.split('\r') {
        if proposals.len() == MAX_PROPOSALS {
            return Err(invalid("too many B2F proposals"));
        }
        let fields = line.split(' ').collect::<Vec<_>>();
        let (kind, id, plain, compressed) = match fields.as_slice() {
            ["FC", kind, id, plain, compressed] | ["FC", kind, id, plain, compressed, "0"] => {
                (*kind, *id, *plain, *compressed)
            }
            _ => return Err(invalid("invalid B2F FC proposal")),
        };
        let proposal = Proposal {
            kind: match kind {
                "EM" => ProposalKind::Message,
                "CM" => ProposalKind::Control,
                _ => return Err(invalid("unknown B2F proposal type")),
            },
            id,
            message_bytes: length(plain)?,
            compressed_bytes: length(compressed)?,
        };
        proposal.validate(max_message_bytes, max_compressed_bytes)?;
        if proposals
            .iter()
            .any(|previous: &Proposal<'_>| previous.id == id)
        {
            return Err(invalid("duplicate B2F proposal ID"));
        }
        proposals.push(proposal);
    }
    Ok(proposals)
}

/// Encode FC proposals using the zero compatibility field used by Pat.
pub fn encode(
    proposals: &[Proposal<'_>],
    max_message_bytes: usize,
    max_compressed_bytes: usize,
) -> Result<String> {
    use std::fmt::Write;

    if !(1..=MAX_PROPOSALS).contains(&proposals.len()) {
        return Err(invalid("invalid B2F proposal count"));
    }
    let mut wire = String::new();
    for (index, proposal) in proposals.iter().enumerate() {
        proposal.validate(max_message_bytes, max_compressed_bytes)?;
        if proposals[..index]
            .iter()
            .any(|previous| previous.id == proposal.id)
        {
            return Err(invalid("duplicate B2F proposal ID"));
        }
        let kind = match proposal.kind {
            ProposalKind::Message => "EM",
            ProposalKind::Control => "CM",
        };
        write!(
            wire,
            "FC {kind} {} {} {} 0\r",
            proposal.id, proposal.message_bytes, proposal.compressed_bytes
        )
        .expect("writing into a String is infallible");
    }
    let sum = checksum(wire.as_bytes());
    write!(wire, "F> {sum:02X}\r").expect("writing into a String is infallible");
    Ok(wire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_answers_keep_each_resume_offset_separate() {
        let proposal = Proposal {
            kind: ProposalKind::Message,
            id: "A",
            message_bytes: 200,
            compressed_bytes: 100,
        };
        let batch = [proposal; 4];
        assert_eq!(
            decode_answers("FS A12!34-h\r", &batch).unwrap(),
            [
                Answer::Accept { offset: 12 },
                Answer::Accept { offset: 34 },
                Answer::Reject,
                Answer::Defer,
            ],
        );
        for invalid in ["FS !\r", "FS !101\r", "FS ++\r", "FS \r", "FS é\r"] {
            assert!(decode_answers(invalid, &[proposal]).is_err());
        }
    }

    #[test]
    fn verifies_reference_proposal() {
        let wire = b"FC EM FOYNU8AKXX59 260 221 0\rF> 68\r";
        let block = decode(wire, 260, 221).unwrap();
        assert_eq!(block[0].id, "FOYNU8AKXX59");
        assert_eq!(block[0].message_bytes, 260);
        assert_eq!(encode(&block, 260, 221).unwrap().as_bytes(), wire);
        assert!(decode(wire, 260, 220).is_err());
        for end in 0..wire.len() {
            assert!(decode(&wire[..end], 260, 221).is_err());
        }
        let mut changed = wire.to_vec();
        changed[6] ^= 1;
        assert!(decode(&changed, 260, 221).is_err());
        assert!(encode(&[block[0]; 2], 260, 221).is_err());
    }
}
