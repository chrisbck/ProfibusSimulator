use std::assert_eq;

const SD1_START_DELIMITER: u8 = 0x10;
const SD4_START_DELIMITER: u8 = 0xDC;
const END_DELIMITER: u8 = 0x16;

struct Telegram {
    name: String,
    bytes: Vec<u8>,
}

impl Telegram {
    fn new(name: &str, bytes: Vec<u8>) -> Self {
        // return a Telegram
        Self {
            // Create an instance of Telegram
            name: name.to_string(),
            bytes, // move Vec<u8> into bytes
        }
    }

    fn new_sd1(da: u8, sa: u8, fc: u8) -> Self {
        let fcs = calculate_fcs(da, sa, fc);

        let bytes = vec![SD1_START_DELIMITER, da, sa, fc, fcs, END_DELIMITER];

        Self::new("SD1", bytes)
    }

    fn new_sd4(da: u8, sa: u8) -> Self {
        let bytes = vec![SD4_START_DELIMITER, da, sa];

        Self::new("SD4", bytes)
    }

    fn print(&self) {
        print!("{}: ", self.name);

        for byte in &self.bytes {
            print!("{byte:02X} ");
        }

        println!();
    }
}

/*
PROFIBUS SD1 frame:

SD1   DA   SA   FC   FCS   ED
10    xx   xx   xx   xx    16

FCS = DA + SA + FC, wrapped to 8 bits
*/
fn calculate_fcs(da: u8, sa: u8, fc: u8) -> u8 {
    da.wrapping_add(sa).wrapping_add(fc)
}

fn main() {
    let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);
    let fcs = calculate_fcs(0x05, 0x02, 0x49);
    let token = Telegram::new_sd4(0x05, 0x02);

    println!("FCS = {fcs:02X}");
    telegram.print();
    token.print();
}

#[test]
fn test_fcs() {
    let fcs = calculate_fcs(0x05, 0x02, 0x49);

    assert_eq!(fcs, 0x50);
}

#[test]
fn test_fcs_wraps_on_overflow() {
    let fcs = calculate_fcs(250, 10, 5);

    assert_eq!(fcs, 9);
}

#[test]
fn test_sd1_frame() {
    let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);

    assert_eq!(telegram.name, "SD1");
    assert_eq!(telegram.bytes, vec![0x10, 0x05, 0x02, 0x49, 0x50, 0x16]);
}

#[test]
fn test_sd4_frame() {
    // create SD4 telegram
    let telegram = Telegram::new_sd4(0x13, 0x42);

    // assert its name
    assert_eq!(telegram.name, "SD4");
    // assert its byte vector
    assert_eq!(telegram.bytes, vec![0xDC, 0x13, 0x42]);
}
