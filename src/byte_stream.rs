use std::time::Duration;

use crate::telegram::Telegram;

pub struct ByteStream {
    telegrams: Vec<Telegram>,
    telegram_index: usize,
    byte_index: usize,
}

impl ByteStream {
    pub fn new(telegrams: Vec<Telegram>) -> Self {
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

        let bytes = self.telegrams[self.telegram_index].bytes();

        if self.byte_index < bytes.len() {
            let byte = bytes[self.byte_index];
            self.byte_index += 1;

            Some(byte)
        } else {
            self.telegram_index += 1;
            self.byte_index = 0;
            self.next()
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct TimedByte {
    pub byte: u8,
    pub timestamp: Duration,
}

pub struct TimedByteStream {
    byte_stream: ByteStream,
    character_time: Duration,
    elapsed_time: Duration,
}

impl TimedByteStream {
    pub fn new(byte_stream: ByteStream, baud_rate: u32) -> Result<Self, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telegram::Telegram;

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
}
