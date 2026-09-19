use crate::telegram::{
    calculate_fcs, Telegram, END_DELIMITER, SD1_START_DELIMITER, SD4_START_DELIMITER,
};

const SD1_FRAME_LEN: usize = 6;
const SD4_FRAME_LEN: usize = 3;

#[derive(Debug, PartialEq, Eq)]
enum ParserState {
    WaitingForStart,
    ReadingSd1,
    ReadingSd4,
}

pub struct Parser {
    state: ParserState,
    buffer: Vec<u8>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            state: ParserState::WaitingForStart,
            buffer: Vec::new(),
        }
    }

    pub fn push(&mut self, byte: u8) -> Option<Telegram> {
        match self.state {
            ParserState::WaitingForStart => {
                if byte == SD1_START_DELIMITER {
                    self.buffer.push(byte);
                    self.state = ParserState::ReadingSd1;
                } else if byte == SD4_START_DELIMITER {
                    self.buffer.push(byte);
                    self.state = ParserState::ReadingSd4;
                }
                None
            }
            ParserState::ReadingSd1 => {
                self.buffer.push(byte);

                if self.buffer.len() < SD1_FRAME_LEN {
                    None
                } else {
                    self.finish_sd1_candidate()
                }
            }
            ParserState::ReadingSd4 => {
                self.buffer.push(byte);

                if self.buffer.len() < SD4_FRAME_LEN {
                    None
                } else {
                    self.finish_sd4_candidate()
                }
            }
        }
    }

    fn finish_sd1_candidate(&mut self) -> Option<Telegram> {
        let candidate = std::mem::take(&mut self.buffer);
        self.state = ParserState::WaitingForStart;

        if Self::validate_sd1(&candidate) {
            return Some(Telegram::new_sd1(candidate[1], candidate[2], candidate[3]));
        }

        self.recover_from(&candidate)
    }

    fn finish_sd4_candidate(&mut self) -> Option<Telegram> {
        let candidate = std::mem::take(&mut self.buffer);
        self.state = ParserState::WaitingForStart;

        // Every three bytes starting with 0xDC are structurally a valid SD4;
        // there's no FCS or end delimiter to check.
        Some(Telegram::new_sd4(candidate[1], candidate[2]))
    }

    fn validate_sd1(candidate: &[u8]) -> bool {
        candidate.len() == SD1_FRAME_LEN
            && candidate[0] == SD1_START_DELIMITER
            && candidate[5] == END_DELIMITER
            && candidate[4] == calculate_fcs(&candidate[1..4])
    }

    // Look for the earliest plausible SD1 or SD4 start inside the failed
    // candidate and replay everything from there through the normal state
    // machine, returning any telegram that replay produces.
    fn recover_from(&mut self, candidate: &[u8]) -> Option<Telegram> {
        let offset = candidate[1..]
            .iter()
            .position(|&b| b == SD1_START_DELIMITER || b == SD4_START_DELIMITER)?;

        let mut recovered = None;

        for &byte in &candidate[offset + 1..] {
            // A failed SD1_FRAME_LEN-byte candidate can only recover to an
            // offset >= 1, so the replayed suffix is at most 5 bytes. That's
            // long enough to complete one SD4 (3 bytes) but never a second
            // frame on top of it, and never a second 6-byte SD1. So at most
            // one telegram can come out of a single replay here; keep the
            // first and keep feeding the rest so no byte is dropped.
            // Revisit this once SD2 recovery is added and replay suffixes
            // can be longer.
            let telegram = self.push(byte);
            recovered = recovered.or(telegram);
        }

        recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_sd1_byte_at_a_time() {
        let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);
        let bytes = telegram.bytes().to_vec();

        let mut parser = Parser::new();

        for &byte in &bytes[..bytes.len() - 1] {
            assert!(parser.push(byte).is_none());
        }

        let result = parser.push(*bytes.last().unwrap());

        let telegram = result.expect("valid SD1 should parse");
        assert_eq!(telegram.bytes(), &[0x10, 0x05, 0x02, 0x49, 0x50, 0x16]);
    }

    #[test]
    fn test_garbage_before_sd1() {
        let mut parser = Parser::new();

        assert!(parser.push(0xAA).is_none());
        assert!(parser.push(0xBB).is_none());
        assert!(parser.push(0xCC).is_none());

        let mut result = None;
        for &byte in Telegram::new_sd1(0x05, 0x02, 0x49).bytes() {
            result = parser.push(byte);
        }

        let telegram = result.expect("SD1 should still parse after leading garbage");
        assert_eq!(telegram.bytes(), &[0x10, 0x05, 0x02, 0x49, 0x50, 0x16]);
    }

    #[test]
    fn test_bad_fcs_rejected() {
        let mut parser = Parser::new();
        let bytes = [0x10, 0x05, 0x02, 0x49, 0xFF, 0x16]; // wrong FCS

        let mut result = None;
        for &byte in &bytes {
            result = parser.push(byte);
        }

        assert!(result.is_none());
    }

    #[test]
    fn test_bad_end_delimiter_rejected() {
        let mut parser = Parser::new();
        let bytes = [0x10, 0x05, 0x02, 0x49, 0x50, 0x00]; // wrong end delimiter

        let mut result = None;
        for &byte in &bytes {
            result = parser.push(byte);
        }

        assert!(result.is_none());
    }

    #[test]
    fn test_delimiter_like_values_inside_frame() {
        let mut parser = Parser::new();
        let source = Telegram::new_sd1(0x10, 0x68, 0xDC);

        let mut result = None;
        for &byte in source.bytes() {
            result = parser.push(byte);
        }

        let parsed = result.expect("SD1 with delimiter-like DA/SA/FC should still parse");
        assert_eq!(parsed.bytes(), source.bytes());
    }

    #[test]
    fn test_recovers_from_embedded_start_delimiter() {
        let mut parser = Parser::new();

        // First candidate: 10 AA 10 22 33 44 -> fails (bad FCS/ED), but
        // contains a real SD1 starting at the embedded 0x10.
        assert!(parser.push(0x10).is_none());
        assert!(parser.push(0xAA).is_none());
        assert!(parser.push(0x10).is_none());
        assert!(parser.push(0x22).is_none());
        assert!(parser.push(0x33).is_none());
        assert!(parser.push(0x44).is_none()); // candidate completes and fails, recovers to "10 22 33 44"

        // The recovered candidate now holds [0x10, 0x22, 0x33, 0x44] and needs
        // two more bytes (FCS, ED) to complete as DA=0x22, SA=0x33, FC=0x44.
        let fcs = calculate_fcs(&[0x22, 0x33, 0x44]);
        assert!(parser.push(fcs).is_none());
        let result = parser.push(END_DELIMITER);

        let telegram = result.expect("parser should recover from the embedded start delimiter");
        assert_eq!(
            telegram.bytes(),
            &[0x10, 0x22, 0x33, 0x44, fcs, END_DELIMITER]
        );
    }

    #[test]
    fn test_two_consecutive_valid_sd1_frames() {
        let mut parser = Parser::new();

        let first = Telegram::new_sd1(0x05, 0x02, 0x49);
        let second = Telegram::new_sd1(0x13, 0x42, 0x7B);

        let mut first_result = None;
        for &byte in first.bytes() {
            first_result = parser.push(byte);
        }

        let mut second_result = None;
        for &byte in second.bytes() {
            second_result = parser.push(byte);
        }

        assert_eq!(
            first_result.expect("first SD1 should parse").bytes(),
            first.bytes()
        );
        assert_eq!(
            second_result.expect("second SD1 should parse").bytes(),
            second.bytes()
        );
    }

    #[test]
    fn test_valid_sd4_byte_at_a_time() {
        let mut parser = Parser::new();

        assert!(parser.push(0xDC).is_none());
        assert!(parser.push(0x05).is_none());

        let result = parser.push(0x02);

        let telegram = result.expect("valid SD4 should parse");
        assert_eq!(telegram.bytes(), &[0xDC, 0x05, 0x02]);
    }

    #[test]
    fn test_garbage_before_sd4() {
        let mut parser = Parser::new();

        assert!(parser.push(0xAA).is_none());
        assert!(parser.push(0xBB).is_none());

        let mut result = None;
        for &byte in Telegram::new_sd4(0x05, 0x02).bytes() {
            result = parser.push(byte);
        }

        let telegram = result.expect("SD4 should still parse after leading garbage");
        assert_eq!(telegram.bytes(), &[0xDC, 0x05, 0x02]);
    }

    #[test]
    fn test_two_consecutive_sd4_frames() {
        let mut parser = Parser::new();

        let first = Telegram::new_sd4(0x05, 0x02);
        let second = Telegram::new_sd4(0x13, 0x42);

        let mut first_result = None;
        for &byte in first.bytes() {
            first_result = parser.push(byte);
        }

        let mut second_result = None;
        for &byte in second.bytes() {
            second_result = parser.push(byte);
        }

        assert_eq!(
            first_result.expect("first SD4 should parse").bytes(),
            first.bytes()
        );
        assert_eq!(
            second_result.expect("second SD4 should parse").bytes(),
            second.bytes()
        );
    }

    #[test]
    fn test_sd1_followed_by_sd4() {
        let mut parser = Parser::new();

        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);
        let sd4 = Telegram::new_sd4(0x13, 0x42);

        let mut sd1_result = None;
        for &byte in sd1.bytes() {
            sd1_result = parser.push(byte);
        }

        let mut sd4_result = None;
        for &byte in sd4.bytes() {
            sd4_result = parser.push(byte);
        }

        assert_eq!(sd1_result.expect("SD1 should parse").bytes(), sd1.bytes());
        assert_eq!(sd4_result.expect("SD4 should parse").bytes(), sd4.bytes());
    }

    #[test]
    fn test_sd4_followed_by_sd1() {
        let mut parser = Parser::new();

        let sd4 = Telegram::new_sd4(0x13, 0x42);
        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);

        let mut sd4_result = None;
        for &byte in sd4.bytes() {
            sd4_result = parser.push(byte);
        }

        let mut sd1_result = None;
        for &byte in sd1.bytes() {
            sd1_result = parser.push(byte);
        }

        assert_eq!(sd4_result.expect("SD4 should parse").bytes(), sd4.bytes());
        assert_eq!(sd1_result.expect("SD1 should parse").bytes(), sd1.bytes());
    }

    #[test]
    fn test_delimiter_like_values_as_sd4_address() {
        let mut parser = Parser::new();
        let source = Telegram::new_sd4(0x10, 0xDC);

        let mut result = None;
        for &byte in source.bytes() {
            result = parser.push(byte);
        }

        let parsed = result.expect("SD4 with delimiter-like DA/SA should still parse");
        assert_eq!(parsed.bytes(), source.bytes());
    }

    #[test]
    fn test_recovers_sd4_from_failed_sd1() {
        let mut parser = Parser::new();

        // Failed SD1 candidate "10 AA DC 05 02 99" contains a valid SD4
        // start (0xDC) before completing as an invalid SD1.
        assert!(parser.push(0x10).is_none());
        assert!(parser.push(0xAA).is_none());
        assert!(parser.push(0xDC).is_none());
        assert!(parser.push(0x05).is_none());
        assert!(parser.push(0x02).is_none());
        let result = parser.push(0x99);

        let telegram = result.expect("SD4 recovered from within a failed SD1 candidate");
        assert_eq!(telegram.bytes(), Telegram::new_sd4(0x05, 0x02).bytes());
    }

    #[test]
    fn test_mixed_stream_garbage_sd1_garbage_sd4_sd1() {
        let mut parser = Parser::new();

        let sd1_a = Telegram::new_sd1(0x05, 0x02, 0x49);
        let sd4 = Telegram::new_sd4(0x13, 0x42);
        let sd1_b = Telegram::new_sd1(0x07, 0x01, 0x22);

        let mut stream = vec![0xAA, 0xBB];
        stream.extend_from_slice(sd1_a.bytes());
        stream.extend_from_slice(&[0xCC, 0xEE]);
        stream.extend_from_slice(sd4.bytes());
        stream.extend_from_slice(sd1_b.bytes());

        let mut results = Vec::new();
        for byte in stream {
            if let Some(telegram) = parser.push(byte) {
                results.push(telegram);
            }
        }

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].bytes(), sd1_a.bytes());
        assert_eq!(results[1].bytes(), sd4.bytes());
        assert_eq!(results[2].bytes(), sd1_b.bytes());
    }
}
