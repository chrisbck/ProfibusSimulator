use std::fmt;
use std::time::Duration;

const SD1_START_DELIMITER: u8 = 0x10;
const SD2_START_DELIMITER: u8 = 0x68;
const SD4_START_DELIMITER: u8 = 0xDC;
const END_DELIMITER: u8 = 0x16;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum FrameType {
    SD1,
    SD2,
    SD4,
}

impl fmt::Display for FrameType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            FrameType::SD1 => "SD1",
            FrameType::SD2 => "SD2",
            FrameType::SD4 => "SD4",
        };

        write!(f, "{name}")
    }
}

struct Telegram {
    frame_type: FrameType,
    bytes: Vec<u8>,
}

struct ByteStream {
    telegrams: Vec<Telegram>,
    telegram_index: usize,
    byte_index: usize,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct TimedByte {
    byte: u8,
    timestamp: Duration,
}

struct TimedByteStream {
    byte_stream: ByteStream,
    character_time: Duration,
    elapsed_time: Duration,
}

impl Telegram {
    fn new(frame_type: FrameType, bytes: Vec<u8>) -> Self {
        // return a Telegram
        Self {
            // Create an instance of Telegram
            frame_type,
            bytes, // move Vec<u8> into bytes
        }
    }

    /*
        PROFIBUS SD1 frame:
        SD1   DA   SA   FC  FCS  ED
        10    xx   xx   xx   xx   16
    */
    fn new_sd1(da: u8, sa: u8, fc: u8) -> Self {
        let fcs = calculate_fcs(&[da, sa, fc]);

        let bytes = vec![SD1_START_DELIMITER, da, sa, fc, fcs, END_DELIMITER];

        Self::new(FrameType::SD1, bytes)
    }

    /*
        PROFIBUS SD2 frame:
        SD2  LE LEr SD2 DA  SA  FC  DATA  FCS ED
        68   xx xx  xx  xx  xx  xx  [xx]  XX  16
    */
    fn new_sd2(da: u8, sa: u8, fc: u8, data: Vec<u8>) -> Result<Self, String> {
        if data.is_empty() {
            return Err("SD2 requires at least one data byte.".to_string());
        }

        if data.len() > 246 {
            return Err("SD2 cannot contain more than 246 data bytes.".to_string());
        }

        // Length of the part covered by the FCS:
        // DA + SA + FC + DATA
        let le: u8 = 3 + data.len() as u8;

        // Build the part covered by the FCS:
        // DA + SA + FC + DATA
        let mut fcs_bytes = vec![da, sa, fc];

        fcs_bytes.extend(&data);

        let fcs = calculate_fcs(&fcs_bytes);

        // Build the complete SD2 telegram
        let mut bytes = vec![SD2_START_DELIMITER, le, le, SD2_START_DELIMITER, da, sa, fc];

        bytes.extend(data);

        bytes.push(fcs);
        bytes.push(END_DELIMITER);

        Ok(Self::new(FrameType::SD2, bytes))
    }

    fn new_sd4(da: u8, sa: u8) -> Self {
        let bytes = vec![SD4_START_DELIMITER, da, sa];

        Self::new(FrameType::SD4, bytes)
    }
}

impl fmt::Display for Telegram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.frame_type)?;

        for byte in &self.bytes {
            write!(f, "{byte:02X} ")?;
        }

        Ok(())
    }
}

impl ByteStream {
    fn new(telegrams: Vec<Telegram>) -> Self {
        Self {
            telegrams,
            telegram_index: 0,
            byte_index: 0,
        }
    }
}

impl Iterator for ByteStream {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.telegram_index >= self.telegrams.len() {
            return None;
        }

        let telegram = &self.telegrams[self.telegram_index];

        if self.byte_index < telegram.bytes.len() {
            let temp_byte = telegram.bytes[self.byte_index];
            self.byte_index += 1;

            Some(temp_byte)
        } else {
            self.telegram_index += 1;
            self.byte_index = 0;
            self.next()
        }
    }
}

impl TimedByteStream {
    fn new(byte_stream: ByteStream, baud_rate: u32) -> Result<Self, String> {
        if baud_rate == 0 {
            return Err("Baud rate must be greater than zero.".to_string());
        }

        let character_time = Duration::from_secs_f64(11.0 / baud_rate as f64);

        Ok(Self {
            byte_stream,
            character_time,
            elapsed_time: Duration::ZERO,
        })
    }
}

impl Iterator for TimedByteStream {
    type Item = TimedByte;

    fn next(&mut self) -> Option<Self::Item> {
        let byte = self.byte_stream.next()?;

        let timed_byte = TimedByte {
            byte,
            timestamp: self.elapsed_time,
        };

        self.elapsed_time += self.character_time;

        Some(timed_byte)
    }
}

/*
Calculate the FCS by summing all supplied bytes,
wrapping the result to 8 bits.
*/
fn calculate_fcs(bytes: &[u8]) -> u8 {
    let mut fcs: u8 = 0;

    for byte in bytes {
        // add byte to fcs using wrapping_add
        fcs = fcs.wrapping_add(*byte);
    }

    fcs
}

fn main() {
    let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);
    let fcs = calculate_fcs(&[0x05, 0x02, 0x49]);
    let token = Telegram::new_sd4(0x05, 0x02);

    println!("FCS = {fcs:02X}");
    println!("{telegram}");
    println!("{token}");
}

#[test]
fn test_fcs() {
    let fcs = calculate_fcs(&[0x05, 0x02, 0x49]);

    assert_eq!(fcs, 0x50);
}

#[test]
fn test_fcs_wraps_on_overflow() {
    let fcs = calculate_fcs(&[250, 10, 5]);

    assert_eq!(fcs, 9);
}

#[test]
fn test_sd1_frame() {
    let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);

    assert_eq!(telegram.frame_type, FrameType::SD1);
    assert_eq!(telegram.bytes, vec![0x10, 0x05, 0x02, 0x49, 0x50, 0x16]);
}

#[test]
fn test_sd2_frame() {
    let telegram = Telegram::new_sd2(0x05, 0x02, 0x49, vec![0x01, 0x02, 0x03])
        .expect("SD2 frame should be valid");

    assert_eq!(telegram.frame_type, FrameType::SD2);

    assert_eq!(
        telegram.bytes,
        vec![0x68, 0x06, 0x06, 0x68, 0x05, 0x02, 0x49, 0x01, 0x02, 0x03, 0x56, 0x16]
    );
}

#[test]
fn test_sd2_frame_no_data() {
    let result = Telegram::new_sd2(0x05, 0x02, 0x49, vec![]);

    assert!(result.is_err());
}

#[test]
fn test_sd2_rejects_too_much_data() {
    let data = vec![0xAA; 247];

    let result = Telegram::new_sd2(0x05, 0x02, 0x49, data);

    assert!(result.is_err());
}

#[test]
fn test_sd2_accepts_maximum_data() {
    let data = vec![0xAA; 246];

    let telegram =
        Telegram::new_sd2(0x05, 0x02, 0x49, data).expect("246 data bytes should be valid");

    assert_eq!(telegram.frame_type, FrameType::SD2);
    assert_eq!(telegram.bytes[0], 0x68);
    assert_eq!(telegram.bytes[1], 0xF9);
    assert_eq!(telegram.bytes[2], 0xF9);
    assert_eq!(telegram.bytes[3], 0x68);
    assert_eq!(telegram.bytes.len(), 255);
    assert_eq!(*telegram.bytes.last().unwrap(), 0x16);
}

#[test]
fn test_sd4_frame() {
    let telegram = Telegram::new_sd4(0x13, 0x42);

    assert_eq!(telegram.frame_type, FrameType::SD4);
    assert_eq!(telegram.bytes, vec![0xDC, 0x13, 0x42]);
}

#[test]
fn test_byte_stream_iterator() {
    let telegrams = vec![
        Telegram::new_sd1(0x05, 0x02, 0x49),
        Telegram::new_sd4(0x05, 0x02),
    ];

    let byte_stream = ByteStream::new(telegrams);

    let bytes: Vec<u8> = byte_stream.collect();

    assert_eq!(
        bytes,
        vec![0x10, 0x05, 0x02, 0x49, 0x50, 0x16, 0xDC, 0x05, 0x02,]
    );
}

#[test]
fn test_empty_byte_stream() {
    let byte_stream = ByteStream::new(vec![]);

    let bytes: Vec<u8> = byte_stream.collect();

    assert!(bytes.is_empty());
}

#[test]
fn test_byte_stream_returns_none_when_finished() {
    let telegrams = vec![Telegram::new_sd4(0x05, 0x02)];

    let mut byte_stream = ByteStream::new(telegrams);

    assert_eq!(byte_stream.next(), Some(0xDC));
    assert_eq!(byte_stream.next(), Some(0x05));
    assert_eq!(byte_stream.next(), Some(0x02));
    assert_eq!(byte_stream.next(), None);
}

#[test]
fn test_timed_byte_stream_character_time() {
    let byte_stream = ByteStream::new(vec![]);

    let timed_stream =
        TimedByteStream::new(byte_stream, 19_200).expect("19.2 kbit/s should be valid");

    assert!(timed_stream.character_time.as_micros() >= 572);
    assert!(timed_stream.character_time.as_micros() <= 573);
}

#[test]
fn test_timed_byte_stream_rejects_zero_baud() {
    let byte_stream = ByteStream::new(vec![]);

    let result = TimedByteStream::new(byte_stream, 0);

    assert!(result.is_err());
}

#[test]
fn test_timed_byte_stream() {
    let telegrams = vec![Telegram::new_sd4(0x05, 0x02)];

    let byte_stream = ByteStream::new(telegrams);

    let timed_stream =
        TimedByteStream::new(byte_stream, 19_200).expect("Baud rate should be valid");

    let timed_bytes: Vec<TimedByte> = timed_stream.collect();

    assert_eq!(timed_bytes.len(), 3);

    assert_eq!(timed_bytes[0].byte, 0xDC);
    assert_eq!(timed_bytes[0].timestamp, Duration::ZERO);

    assert_eq!(timed_bytes[1].byte, 0x05);
    assert_eq!(timed_bytes[2].byte, 0x02);
}

#[test]
fn test_timed_byte_stream_timestamps() {
    let telegrams = vec![Telegram::new_sd4(0x05, 0x02)];

    let byte_stream = ByteStream::new(telegrams);

    let timed_stream =
        TimedByteStream::new(byte_stream, 19_200).expect("Baud rate should be valid");

    let character_time = timed_stream.character_time;

    let timed_bytes: Vec<TimedByte> = timed_stream.collect();

    assert_eq!(timed_bytes[0].timestamp, Duration::ZERO);
    assert_eq!(timed_bytes[1].timestamp, character_time);
    assert_eq!(timed_bytes[2].timestamp, character_time + character_time);
}
