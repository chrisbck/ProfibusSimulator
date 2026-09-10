const SD1_START_DELIMITER: u8 = 0x10;
const SD2_START_DELIMITER: u8 = 0x68;
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
        let fcs = calculate_fcs(&[da, sa, fc]);

        let bytes = vec![SD1_START_DELIMITER, da, sa, fc, fcs, END_DELIMITER];

        Self::new("SD1", bytes)
    }

    fn new_sd2(da: u8, sa: u8, fc: u8, data: Vec<u8>) -> Self {
        let le: u8 = 3 + data.len() as u8;

        // Build the part covered by the FCS:
        // DA + SA + FC + DATA
        let mut fcs_bytes = vec![da, sa, fc];

        fcs_bytes.extend(&data); // concat data to fcs_bytes

        let fcs = calculate_fcs(&fcs_bytes);

        // Build the complete SD2 telegram
        let mut bytes = vec![SD2_START_DELIMITER, le, le, SD2_START_DELIMITER, da, sa, fc];

        bytes.extend(data);

        bytes.push(fcs); //  add the FCS byte to the end of the telegram
        bytes.push(END_DELIMITER); // add the END_DELIMITER byte to the end of the telegram

        Self::new("SD2", bytes)
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
    telegram.print();
    token.print();
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
