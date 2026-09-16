use std::fmt;

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
            return Err("SD2 cannot contain more than 246 bytes.".to_string());
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

    fn print(&self) {
        print!("{}:", self.frame_type);

        for byte in &self.bytes {
            print!("{byte:02X} ");
        }

        println!();
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
