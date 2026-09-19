use crate::telegram::{
    calculate_fcs, Telegram, END_DELIMITER, SD1_START_DELIMITER, SD2_LE_OVERHEAD, SD2_MAX_DATA_LEN,
    SD2_MIN_DATA_LEN, SD2_START_DELIMITER, SD4_START_DELIMITER,
};

const SD1_FRAME_LEN: usize = 6;
const SD4_FRAME_LEN: usize = 3;

// SD2: 68 LE LE 68 | DA SA FC DATA... (LE bytes) | FCS 16
const SD2_HEADER_LEN: usize = 4;
const SD2_FRAME_OVERHEAD: usize = SD2_HEADER_LEN + 2; // header + FCS + ED
const SD2_MIN_LE: u8 = (SD2_LE_OVERHEAD + SD2_MIN_DATA_LEN) as u8;
const SD2_MAX_LE: u8 = (SD2_LE_OVERHEAD + SD2_MAX_DATA_LEN) as u8;

#[derive(Debug, PartialEq, Eq)]
enum ParserState {
    WaitingForStart,
    ReadingSd1,
    ReadingSd2,
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

    // Returns every telegram completed by this byte: usually none or one, but
    // more when a failed candidate is replayed and contains several frames.
    //
    // There is deliberately no timeout: an incomplete candidate is kept until
    // later bytes complete or break it. Abandoning stalled candidates belongs
    // with the timed-stream integration.
    pub fn push(&mut self, byte: u8) -> Vec<Telegram> {
        match self.state {
            ParserState::WaitingForStart => {
                self.start_candidate(byte);
                Vec::new()
            }
            ParserState::ReadingSd1 => {
                self.buffer.push(byte);

                if self.buffer.len() < SD1_FRAME_LEN {
                    Vec::new()
                } else {
                    self.finish_sd1_candidate()
                }
            }
            ParserState::ReadingSd2 => {
                self.buffer.push(byte);
                self.advance_sd2_candidate()
            }
            ParserState::ReadingSd4 => {
                self.buffer.push(byte);

                if self.buffer.len() < SD4_FRAME_LEN {
                    Vec::new()
                } else {
                    self.finish_sd4_candidate()
                }
            }
        }
    }

    fn start_candidate(&mut self, byte: u8) {
        let state = match byte {
            SD1_START_DELIMITER => ParserState::ReadingSd1,
            SD2_START_DELIMITER => ParserState::ReadingSd2,
            SD4_START_DELIMITER => ParserState::ReadingSd4,
            _ => return,
        };

        self.buffer.push(byte);
        self.state = state;
    }

    fn is_start_delimiter(byte: u8) -> bool {
        matches!(
            byte,
            SD1_START_DELIMITER | SD2_START_DELIMITER | SD4_START_DELIMITER
        )
    }

    fn take_candidate(&mut self) -> Vec<u8> {
        self.state = ParserState::WaitingForStart;
        std::mem::take(&mut self.buffer)
    }

    fn finish_sd1_candidate(&mut self) -> Vec<Telegram> {
        let candidate = self.take_candidate();

        if Self::validate_sd1(&candidate) {
            return vec![Telegram::new_sd1(candidate[1], candidate[2], candidate[3])];
        }

        self.recover_from(&candidate)
    }

    fn finish_sd4_candidate(&mut self) -> Vec<Telegram> {
        let candidate = self.take_candidate();

        // Every three bytes starting with 0xDC are structurally a valid SD4;
        // there's no FCS or end delimiter to check.
        vec![Telegram::new_sd4(candidate[1], candidate[2])]
    }

    // Checks the SD2 header as soon as the bytes it covers have arrived, so an
    // impossible header fails immediately instead of waiting for LE + 6 bytes.
    fn advance_sd2_candidate(&mut self) -> Vec<Telegram> {
        let len = self.buffer.len();

        let header_is_bad = match len {
            3 => !Self::sd2_le_is_valid(self.buffer[1], self.buffer[2]),
            4 => self.buffer[3] != SD2_START_DELIMITER,
            _ => false,
        };

        if header_is_bad {
            let candidate = self.take_candidate();
            return self.recover_from(&candidate);
        }

        // Past this point the header is valid, so LE is known to be in range.
        if len >= SD2_HEADER_LEN && len == self.buffer[1] as usize + SD2_FRAME_OVERHEAD {
            return self.finish_sd2_candidate();
        }

        Vec::new()
    }

    fn finish_sd2_candidate(&mut self) -> Vec<Telegram> {
        let candidate = self.take_candidate();

        match Self::parse_sd2(&candidate) {
            Some(telegram) => vec![telegram],
            None => self.recover_from(&candidate),
        }
    }

    fn validate_sd1(candidate: &[u8]) -> bool {
        candidate.len() == SD1_FRAME_LEN
            && candidate[0] == SD1_START_DELIMITER
            && candidate[5] == END_DELIMITER
            && candidate[4] == calculate_fcs(&candidate[1..4])
    }

    fn sd2_le_is_valid(le: u8, le_repeated: u8) -> bool {
        le == le_repeated && (SD2_MIN_LE..=SD2_MAX_LE).contains(&le)
    }

    fn validate_sd2(candidate: &[u8]) -> bool {
        if candidate.len() < SD2_HEADER_LEN {
            return false;
        }

        let len = candidate.len();

        Self::sd2_le_is_valid(candidate[1], candidate[2])
            && candidate[0] == SD2_START_DELIMITER
            && candidate[3] == SD2_START_DELIMITER
            && len == candidate[1] as usize + SD2_FRAME_OVERHEAD
            && candidate[len - 1] == END_DELIMITER
            && candidate[len - 2] == calculate_fcs(&candidate[4..len - 2])
    }

    fn parse_sd2(candidate: &[u8]) -> Option<Telegram> {
        if !Self::validate_sd2(candidate) {
            return None;
        }

        let data_end = candidate.len() - 2;

        // A validated LE guarantees a data length new_sd2 accepts, but treat
        // an error as an invalid candidate rather than unwrapping.
        Telegram::new_sd2(
            candidate[4],
            candidate[5],
            candidate[6],
            candidate[7..data_end].to_vec(),
        )
        .ok()
    }

    // Look for the earliest plausible SD1/SD2/SD4 start after the first byte of
    // the failed candidate and replay everything from there through the normal
    // state machine, collecting every telegram that replay produces.
    //
    // Recovery always restarts from a later byte in the failed candidate,
    // so each nested candidate is shorter than the candidate that caused it.
    // This guarantees termination. Recovery may reprocess bytes, but frame
    // candidates are bounded by the maximum PROFIBUS frame length.
    fn recover_from(&mut self, candidate: &[u8]) -> Vec<Telegram> {
        let Some(offset) = candidate[1..]
            .iter()
            .position(|&b| Self::is_start_delimiter(b))
        else {
            return Vec::new();
        };

        let mut recovered = Vec::new();

        for &byte in &candidate[offset + 1..] {
            recovered.extend(self.push(byte));
        }

        recovered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(parser: &mut Parser, bytes: &[u8]) -> Vec<Telegram> {
        let mut out = Vec::new();

        for &byte in bytes {
            out.extend(parser.push(byte));
        }

        out
    }

    fn parse_all(bytes: &[u8]) -> Vec<Telegram> {
        feed(&mut Parser::new(), bytes)
    }

    fn concat(telegrams: &[&Telegram]) -> Vec<u8> {
        telegrams
            .iter()
            .flat_map(|t| t.bytes().iter().copied())
            .collect()
    }

    fn assert_parsed(actual: &[Telegram], expected: &[&Telegram]) {
        let actual: Vec<&[u8]> = actual.iter().map(|t| t.bytes()).collect();
        let expected: Vec<&[u8]> = expected.iter().map(|t| t.bytes()).collect();

        assert_eq!(actual, expected);
    }

    // Feeds all but the last byte (asserting nothing is produced), then
    // returns whatever the final byte produces.
    fn feed_expecting_output_only_on_last_byte(bytes: &[u8]) -> Vec<Telegram> {
        let mut parser = Parser::new();
        let (last, rest) = bytes.split_last().expect("bytes must not be empty");

        for &byte in rest {
            assert!(parser.push(byte).is_empty());
        }

        parser.push(*last)
    }

    fn sd2(da: u8, sa: u8, fc: u8, data: Vec<u8>) -> Telegram {
        Telegram::new_sd2(da, sa, fc, data).expect("test SD2 should be valid")
    }

    fn with_bad_fcs(telegram: &Telegram) -> Vec<u8> {
        let mut bytes = telegram.bytes().to_vec();
        let fcs_index = bytes.len() - 2;
        bytes[fcs_index] = bytes[fcs_index].wrapping_add(1);
        bytes
    }

    fn with_bad_end_delimiter(telegram: &Telegram) -> Vec<u8> {
        let mut bytes = telegram.bytes().to_vec();
        let last = bytes.len() - 1;
        bytes[last] = 0x00;
        bytes
    }

    // ---- SD1 ----

    #[test]
    fn test_valid_sd1_byte_at_a_time() {
        let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);
        let bytes = telegram.bytes().to_vec();

        let mut parser = Parser::new();

        for &byte in &bytes[..bytes.len() - 1] {
            assert!(parser.push(byte).is_empty());
        }

        let result = parser.push(*bytes.last().unwrap());

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].bytes(), &[0x10, 0x05, 0x02, 0x49, 0x50, 0x16]);
    }

    #[test]
    fn test_garbage_before_sd1() {
        let mut parser = Parser::new();

        assert!(parser.push(0xAA).is_empty());
        assert!(parser.push(0xBB).is_empty());
        assert!(parser.push(0xCC).is_empty());

        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);
        let result = feed(&mut parser, sd1.bytes());

        assert_parsed(&result, &[&sd1]);
    }

    #[test]
    fn test_bad_fcs_rejected() {
        let bytes = [0x10, 0x05, 0x02, 0x49, 0xFF, 0x16]; // wrong FCS

        assert!(parse_all(&bytes).is_empty());
    }

    #[test]
    fn test_bad_end_delimiter_rejected() {
        let bytes = [0x10, 0x05, 0x02, 0x49, 0x50, 0x00]; // wrong end delimiter

        assert!(parse_all(&bytes).is_empty());
    }

    #[test]
    fn test_delimiter_like_values_inside_frame() {
        let source = Telegram::new_sd1(0x10, 0x68, 0xDC);

        let result = parse_all(source.bytes());

        assert_parsed(&result, &[&source]);
    }

    #[test]
    fn test_recovers_from_embedded_start_delimiter() {
        let mut parser = Parser::new();

        // First candidate: 10 AA 10 22 33 44 -> fails (bad FCS/ED), but
        // contains a real SD1 starting at the embedded 0x10.
        assert!(parser.push(0x10).is_empty());
        assert!(parser.push(0xAA).is_empty());
        assert!(parser.push(0x10).is_empty());
        assert!(parser.push(0x22).is_empty());
        assert!(parser.push(0x33).is_empty());
        assert!(parser.push(0x44).is_empty()); // candidate completes and fails, recovers to "10 22 33 44"

        // The recovered candidate now holds [0x10, 0x22, 0x33, 0x44] and needs
        // two more bytes (FCS, ED) to complete as DA=0x22, SA=0x33, FC=0x44.
        let fcs = calculate_fcs(&[0x22, 0x33, 0x44]);
        assert!(parser.push(fcs).is_empty());
        let result = parser.push(END_DELIMITER);

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].bytes(),
            &[0x10, 0x22, 0x33, 0x44, fcs, END_DELIMITER]
        );
    }

    #[test]
    fn test_two_consecutive_valid_sd1_frames() {
        let first = Telegram::new_sd1(0x05, 0x02, 0x49);
        let second = Telegram::new_sd1(0x13, 0x42, 0x7B);

        let result = parse_all(&concat(&[&first, &second]));

        assert_parsed(&result, &[&first, &second]);
    }

    // ---- SD4 ----

    #[test]
    fn test_valid_sd4_byte_at_a_time() {
        let mut parser = Parser::new();

        assert!(parser.push(0xDC).is_empty());
        assert!(parser.push(0x05).is_empty());

        let result = parser.push(0x02);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].bytes(), &[0xDC, 0x05, 0x02]);
    }

    #[test]
    fn test_garbage_before_sd4() {
        let mut parser = Parser::new();

        assert!(parser.push(0xAA).is_empty());
        assert!(parser.push(0xBB).is_empty());

        let sd4 = Telegram::new_sd4(0x05, 0x02);
        let result = feed(&mut parser, sd4.bytes());

        assert_parsed(&result, &[&sd4]);
    }

    #[test]
    fn test_two_consecutive_sd4_frames() {
        let first = Telegram::new_sd4(0x05, 0x02);
        let second = Telegram::new_sd4(0x13, 0x42);

        let result = parse_all(&concat(&[&first, &second]));

        assert_parsed(&result, &[&first, &second]);
    }

    #[test]
    fn test_sd1_followed_by_sd4() {
        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);
        let sd4 = Telegram::new_sd4(0x13, 0x42);

        let result = parse_all(&concat(&[&sd1, &sd4]));

        assert_parsed(&result, &[&sd1, &sd4]);
    }

    #[test]
    fn test_sd4_followed_by_sd1() {
        let sd4 = Telegram::new_sd4(0x13, 0x42);
        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);

        let result = parse_all(&concat(&[&sd4, &sd1]));

        assert_parsed(&result, &[&sd4, &sd1]);
    }

    #[test]
    fn test_delimiter_like_values_as_sd4_address() {
        let source = Telegram::new_sd4(0x10, 0xDC);

        let result = parse_all(source.bytes());

        assert_parsed(&result, &[&source]);
    }

    #[test]
    fn test_recovers_sd4_from_failed_sd1() {
        let mut parser = Parser::new();

        // Failed SD1 candidate "10 AA DC 05 02 99" contains a valid SD4
        // start (0xDC) before completing as an invalid SD1.
        assert!(parser.push(0x10).is_empty());
        assert!(parser.push(0xAA).is_empty());
        assert!(parser.push(0xDC).is_empty());
        assert!(parser.push(0x05).is_empty());
        assert!(parser.push(0x02).is_empty());
        let result = parser.push(0x99);

        assert_parsed(&result, &[&Telegram::new_sd4(0x05, 0x02)]);
    }

    #[test]
    fn test_mixed_stream_garbage_sd1_garbage_sd4_sd1() {
        let sd1_a = Telegram::new_sd1(0x05, 0x02, 0x49);
        let sd4 = Telegram::new_sd4(0x13, 0x42);
        let sd1_b = Telegram::new_sd1(0x07, 0x01, 0x22);

        let mut stream = vec![0xAA, 0xBB];
        stream.extend_from_slice(sd1_a.bytes());
        stream.extend_from_slice(&[0xCC, 0xEE]);
        stream.extend_from_slice(sd4.bytes());
        stream.extend_from_slice(sd1_b.bytes());

        let results = parse_all(&stream);

        assert_parsed(&results, &[&sd1_a, &sd4, &sd1_b]);
    }

    // ---- SD2 parsing ----

    #[test]
    fn test_valid_sd2_byte_at_a_time() {
        let telegram = sd2(0x05, 0x02, 0x49, vec![0x01, 0x02, 0x03]);

        let result = feed_expecting_output_only_on_last_byte(telegram.bytes());

        assert_parsed(&result, &[&telegram]);
    }

    #[test]
    fn test_valid_sd2_minimum_payload() {
        let telegram = sd2(0x05, 0x02, 0x49, vec![0x01]);

        let result = feed_expecting_output_only_on_last_byte(telegram.bytes());

        assert_parsed(&result, &[&telegram]);
    }

    #[test]
    fn test_valid_sd2_larger_payload() {
        let telegram = sd2(0x05, 0x02, 0x49, (0x20u8..0x48).collect());

        let result = feed_expecting_output_only_on_last_byte(telegram.bytes());

        assert_parsed(&result, &[&telegram]);
    }

    #[test]
    fn test_valid_sd2_maximum_payload() {
        let telegram = sd2(0x05, 0x02, 0x49, vec![0xAA; 246]);
        assert_eq!(telegram.bytes().len(), 255);

        let result = feed_expecting_output_only_on_last_byte(telegram.bytes());

        assert_parsed(&result, &[&telegram]);
    }

    #[test]
    fn test_garbage_before_sd2() {
        let mut parser = Parser::new();

        assert!(parser.push(0xAA).is_empty());
        assert!(parser.push(0xBB).is_empty());
        assert!(parser.push(0xCC).is_empty());

        let telegram = sd2(0x05, 0x02, 0x49, vec![0x01, 0x02, 0x03]);
        let result = feed(&mut parser, telegram.bytes());

        assert_parsed(&result, &[&telegram]);
    }

    #[test]
    fn test_sd2_le_mismatch_rejected_early() {
        let mut parser = Parser::new();

        assert!(parser.push(0x68).is_empty());
        assert!(parser.push(0x05).is_empty());
        assert_eq!(parser.state, ParserState::ReadingSd2);

        // Second LE differs: rejected on the third byte, not after LE + 6.
        assert!(parser.push(0x06).is_empty());
        assert_eq!(parser.state, ParserState::WaitingForStart);
    }

    #[test]
    fn test_sd2_bad_repeated_start_delimiter_rejected_early() {
        let mut parser = Parser::new();

        assert!(feed(&mut parser, &[0x68, 0xF0, 0xF0]).is_empty());
        assert_eq!(parser.state, ParserState::ReadingSd2);

        // Rejected on the fourth byte, not after waiting for 246 bytes.
        assert!(parser.push(0x00).is_empty());
        assert_eq!(parser.state, ParserState::WaitingForStart);
    }

    #[test]
    fn test_sd2_le_out_of_range_rejected() {
        // Supported LE range is 4..=249 (1..=246 data bytes + DA/SA/FC).
        for le in [0x00, 0x03, 250, 0xFF] {
            let mut parser = Parser::new();

            assert!(feed(&mut parser, &[0x68, le]).is_empty());
            assert!(parser.push(le).is_empty());
            assert_eq!(parser.state, ParserState::WaitingForStart, "LE = {le}");
        }
    }

    #[test]
    fn test_sd2_le_boundaries_accepted() {
        for le in [4u8, 249] {
            let mut parser = Parser::new();

            assert!(feed(&mut parser, &[0x68, le, le, 0x68]).is_empty());
            assert_eq!(parser.state, ParserState::ReadingSd2, "LE = {le}");
        }
    }

    #[test]
    fn test_sd2_bad_fcs_rejected() {
        let telegram = sd2(0x05, 0x02, 0x49, vec![0x01, 0x02, 0x03]);

        assert!(parse_all(&with_bad_fcs(&telegram)).is_empty());
    }

    #[test]
    fn test_sd2_bad_end_delimiter_rejected() {
        let telegram = sd2(0x05, 0x02, 0x49, vec![0x01, 0x02, 0x03]);

        assert!(parse_all(&with_bad_end_delimiter(&telegram)).is_empty());
    }

    #[test]
    fn test_delimiter_like_values_inside_sd2_data() {
        // DA, SA, FC and DATA all contain start/end delimiter values; none of
        // them may restart the candidate.
        let telegram = sd2(
            0x10,
            0x68,
            0xDC,
            vec![0x10, 0x68, 0xDC, 0x16, 0x68, 0x10, 0xDC],
        );

        let result = feed_expecting_output_only_on_last_byte(telegram.bytes());

        assert_parsed(&result, &[&telegram]);
    }

    #[test]
    fn test_incomplete_sd2_candidate_is_retained() {
        let mut parser = Parser::new();

        // No timeout yet: a truncated frame just stays pending.
        assert!(feed(&mut parser, &[0x68, 0x0A, 0x0A, 0x68, 0x05, 0x02]).is_empty());
        assert_eq!(parser.state, ParserState::ReadingSd2);
    }

    // ---- SD2 in mixed streams ----

    #[test]
    fn test_sd1_followed_by_sd2() {
        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);
        let sd2 = sd2(0x13, 0x42, 0x7B, vec![0x01, 0x02, 0x03]);

        let result = parse_all(&concat(&[&sd1, &sd2]));

        assert_parsed(&result, &[&sd1, &sd2]);
    }

    #[test]
    fn test_sd2_followed_by_sd4() {
        let sd2 = sd2(0x13, 0x42, 0x7B, vec![0x01, 0x02, 0x03]);
        let sd4 = Telegram::new_sd4(0x05, 0x02);

        let result = parse_all(&concat(&[&sd2, &sd4]));

        assert_parsed(&result, &[&sd2, &sd4]);
    }

    #[test]
    fn test_sd4_then_sd2_then_sd1() {
        let sd4 = Telegram::new_sd4(0x05, 0x02);
        let sd2 = sd2(0x13, 0x42, 0x7B, vec![0x01, 0x02, 0x03]);
        let sd1 = Telegram::new_sd1(0x07, 0x01, 0x22);

        let result = parse_all(&concat(&[&sd4, &sd2, &sd1]));

        assert_parsed(&result, &[&sd4, &sd2, &sd1]);
    }

    // ---- SD2 recovery ----
    //
    // Each failed outer SD2 below is 68 LE LE 68 05 02 49 <data> FCS ED with a
    // corrupted FCS or ED. Its earliest recovery point is the repeated 0x68 at
    // index 3, which replays as a (bad-header) SD2 attempt with LE=05 / LE=02.
    // That fails early and the replay then reaches the embedded frames.

    #[test]
    fn test_failed_sd2_containing_valid_sd1() {
        let inner = Telegram::new_sd1(0x11, 0x12, 0x13);
        let outer = sd2(0x05, 0x02, 0x49, inner.bytes().to_vec());

        let result = feed_expecting_output_only_on_last_byte(&with_bad_fcs(&outer));

        assert_parsed(&result, &[&inner]);
    }

    #[test]
    fn test_failed_sd2_containing_valid_sd4() {
        let inner = Telegram::new_sd4(0x21, 0x22);
        let outer = sd2(0x05, 0x02, 0x49, inner.bytes().to_vec());

        let result = feed_expecting_output_only_on_last_byte(&with_bad_end_delimiter(&outer));

        assert_parsed(&result, &[&inner]);
    }

    #[test]
    fn test_failed_sd2_containing_valid_sd2() {
        let inner = sd2(0x01, 0x02, 0x03, vec![0x41, 0x42]);
        let outer = sd2(0x05, 0x02, 0x49, inner.bytes().to_vec());

        let result = feed_expecting_output_only_on_last_byte(&with_bad_end_delimiter(&outer));

        assert_parsed(&result, &[&inner]);
    }

    #[test]
    fn test_failed_sd2_recovers_multiple_telegrams_in_order() {
        let sd1_a = Telegram::new_sd1(0x11, 0x12, 0x13);
        let sd4 = Telegram::new_sd4(0x21, 0x22);
        let sd1_b = Telegram::new_sd1(0x31, 0x32, 0x33);

        let outer = sd2(0x05, 0x02, 0x49, concat(&[&sd1_a, &sd4, &sd1_b]));
        assert_eq!(outer.bytes().len(), 24);

        // Everything is produced by the single push of the final byte.
        let result = feed_expecting_output_only_on_last_byte(&with_bad_fcs(&outer));

        assert_parsed(&result, &[&sd1_a, &sd4, &sd1_b]);
    }

    #[test]
    fn test_nested_recovery_inside_failed_sd2() {
        // The replay of the failed SD2 hits a second failed candidate
        // (10 AA DC 05 02 99, a bad SD1 hiding an SD4) whose own recovery
        // must run before the trailing SD1 is parsed.
        let hidden_sd4 = Telegram::new_sd4(0x05, 0x02);
        let trailing_sd1 = Telegram::new_sd1(0x11, 0x12, 0x13);

        let mut data = vec![0x10, 0xAA, 0xDC, 0x05, 0x02, 0x99];
        data.extend_from_slice(trailing_sd1.bytes());

        let outer = sd2(0x05, 0x02, 0x49, data);

        let result = feed_expecting_output_only_on_last_byte(&with_bad_fcs(&outer));

        assert_parsed(&result, &[&hidden_sd4, &trailing_sd1]);
    }

    #[test]
    fn test_malformed_sd2_header_then_valid_frame() {
        let sd1 = Telegram::new_sd1(0x05, 0x02, 0x49);

        // Each bad header fails as soon as it can, so the following SD1 is
        // reported on its own last byte. If the parser instead waited for the
        // claimed SD2 length, the SD1 would be swallowed or delayed.
        let bad_headers: [&[u8]; 3] = [
            &[0x68, 0xF0, 0xF0, 0x00], // repeated delimiter wrong, LE=240
            &[0x68, 0x05, 0x06],       // LE mismatch
            &[0x68, 0x0A, 0x0A, 0x69], // repeated delimiter 0x69
        ];

        for header in bad_headers {
            let mut stream = header.to_vec();
            stream.extend_from_slice(sd1.bytes());

            let result = feed_expecting_output_only_on_last_byte(&stream);

            assert_parsed(&result, &[&sd1]);
        }
    }

    #[test]
    fn test_parser_terminates_on_adversarial_streams() {
        let mut streams: Vec<Vec<u8>> = vec![
            vec![0x68; 3000],
            vec![0x10; 3000],
            vec![0xDC; 3000],
            [0x68, 0x10, 0xDC, 0x16].repeat(1000),
        ];

        // Deterministic pseudo-random stream heavily biased toward delimiters.
        let mut x: u32 = 12345;
        let mut noisy = Vec::new();
        for _ in 0..20_000 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let pick = (x >> 24) as u8;
            noisy.push(match pick % 8 {
                0 => 0x10,
                1 => 0x68,
                2 => 0xDC,
                3 => 0x16,
                _ => (x >> 16) as u8,
            });
        }
        streams.push(noisy);

        for stream in &streams {
            let telegrams = parse_all(stream);

            // Whatever was recovered must be a genuine, canonical frame.
            for telegram in &telegrams {
                let reparsed = parse_all(telegram.bytes());
                assert_parsed(&reparsed, &[telegram]);
            }
        }
    }
}
