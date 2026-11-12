//! Caller identity frame for the VARA HF link setup.
//!
//! Layout follows §4.2A of Saul St John's published VARA HF protocol notes.
//! This module handles the frame bytes; the level-4 waveform is a separate
//! responsibility.

use super::coding;

const CALLER_SLOTS: usize = 6;
const CONTROL: u8 = 0x82;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Bw500,
    Wide,
}

impl Profile {
    fn body_len(self) -> usize {
        match self {
            Self::Bw500 => 44,
            Self::Wide => 90,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSetup {
    pub caller: String,
    pub high_speed: bool,
    pub requested_level: u8,
}

impl LinkSetup {
    /// Builds the CRC-protected frame for a stock-compatible level request.
    pub fn encode(&self, profile: Profile) -> Option<Vec<u8>> {
        if !(1..=if self.high_speed { 6 } else { 4 }).contains(&self.requested_level) {
            return None;
        }
        let caller = self.caller.to_ascii_uppercase();
        let (base, ssid) = match caller.split_once('-') {
            Some((base, suffix)) => {
                let ssid = suffix.parse::<u8>().ok()?;
                if !(1..=15).contains(&ssid) || suffix != ssid.to_string() {
                    return None;
                }
                (base, ssid)
            }
            None => (caller.as_str(), 0),
        };
        if base.is_empty() || base.len() > CALLER_SLOTS {
            return None;
        }

        // Six-bit symbols occupy the most significant bits of the five bytes.
        let mut packed = 0_u64;
        for (i, byte) in base.bytes().enumerate() {
            let symbol = match byte {
                b'A'..=b'Z' => byte - b'A' + 1,
                b'0'..=b'9' => byte - b'0' + 27,
                _ => return None,
            };
            packed |= u64::from(symbol) << (34 - 6 * i);
        }
        let body_len = profile.body_len();
        let mut body = vec![0; body_len];
        body[..5].copy_from_slice(&packed.to_be_bytes()[3..]);
        body[5] = ssid;
        body[7] = 0x80 | u8::from(self.high_speed);
        body[8] = 0x14;
        body[9] = coding::crc(caller.as_bytes()).to_be_bytes()[0];
        body[body_len - 2] = self.requested_level;
        body[body_len - 1] = CONTROL;
        body.extend(coding::crc(&body).to_be_bytes());
        Some(body)
    }

    /// Validates and extracts the caller, capability and level request.
    pub fn decode(profile: Profile, frame: &[u8]) -> Option<Self> {
        let body_len = profile.body_len();
        if frame.len() != body_len + 2 {
            return None;
        }
        let (body, crc) = frame.split_at(body_len);
        let control = body[body_len - 1];
        let stock_bw500_control = profile == Profile::Bw500 && control == 0x02;
        if coding::crc(body).to_be_bytes() != crc
            || body[4] & 0x0f != 0
            || body[5] > 15
            || body[6] != 0
            || !matches!(body[7], 0x80 | 0x81)
            || body[8] != 0x14
            || body[10..body_len - 2].contains(&0x14)
            || !(control == CONTROL || stock_bw500_control)
        {
            return None;
        }
        let high_speed = body[7] == 0x81;
        let requested_level = body[body_len - 2];
        if !(1..=if high_speed { 6 } else { 4 }).contains(&requested_level) {
            return None;
        }
        let packed = body[..5]
            .iter()
            .fold(0_u64, |bits, &byte| (bits << 8) | u64::from(byte));
        let mut caller = String::new();
        let mut ended = false;
        for i in 0..CALLER_SLOTS {
            let symbol = ((packed >> (34 - 6 * i)) & 0x3f) as u8;
            let byte = match symbol {
                0 => {
                    ended = true;
                    continue;
                }
                1..=26 if !ended => b'A' + symbol - 1,
                27..=36 if !ended => b'0' + symbol - 27,
                _ => return None,
            };
            caller.push(char::from(byte));
        }
        if caller.is_empty() {
            return None;
        }
        if body[5] != 0 {
            caller.push('-');
            caller.push_str(&body[5].to_string());
        }
        if coding::crc(caller.as_bytes()).to_be_bytes()[0] != body[9] {
            return None;
        }
        Some(Self {
            caller,
            high_speed,
            requested_level,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_published_stock_setup_frames() {
        for (caller, profile, high_speed, head, tail) in [
            (
                "W9SSJ",
                Profile::Bw500,
                false,
                [0x5e, 0x44, 0xd3, 0x28],
                [0x04, 0x82, 0x1d, 0xee],
            ),
            (
                "AAAA1",
                Profile::Wide,
                false,
                [0x04, 0x10, 0x41, 0x70],
                [0x04, 0x82, 0x53, 0x87],
            ),
            (
                "N0CALL",
                Profile::Bw500,
                true,
                [0x39, 0xb0, 0xc1, 0x30],
                [0x06, 0x82, 0x2c, 0xfb],
            ),
        ] {
            let setup = LinkSetup {
                caller: caller.into(),
                high_speed,
                requested_level: tail[0],
            };
            let mut frame = setup.encode(profile).unwrap();
            assert_eq!(&frame[..4], &head);
            assert_eq!(&frame[frame.len() - 4..], &tail);
            assert_eq!(LinkSetup::decode(profile, &frame), Some(setup));
            frame[9] ^= 1;
            assert_eq!(LinkSetup::decode(profile, &frame), None);
        }
    }

    #[test]
    fn accepts_stock_bw500_setup_with_0x02_control() {
        let setup = LinkSetup {
            caller: "KN6MQS".into(),
            high_speed: false,
            requested_level: 4,
        };
        let mut frame = setup.encode(Profile::Bw500).unwrap();
        frame[43] = 0x02;
        let crc = coding::crc(&frame[..44]).to_be_bytes();
        frame[44..].copy_from_slice(&crc);
        assert_eq!(LinkSetup::decode(Profile::Bw500, &frame), Some(setup));
        frame[9] ^= 1;
        assert!(LinkSetup::decode(Profile::Bw500, &frame).is_none());
    }

    #[test]
    fn preserves_the_caller_ssid() {
        let setup = LinkSetup {
            caller: "W9SSJ-10".into(),
            high_speed: false,
            requested_level: 4,
        };
        let frame = setup.encode(Profile::Bw500).unwrap();
        assert_eq!(frame[5], 10);
        assert_eq!(frame[9], 0x37);
        assert_eq!(LinkSetup::decode(Profile::Bw500, &frame), Some(setup));
    }
}
