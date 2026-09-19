mod byte_stream;
mod parser;
mod telegram;

use telegram::{calculate_fcs, Telegram};

fn main() {
    let telegram = Telegram::new_sd1(0x05, 0x02, 0x49);
    let fcs = calculate_fcs(&[0x05, 0x02, 0x49]);
    let token = Telegram::new_sd4(0x05, 0x02);

    println!("FCS = {fcs:02X}");
    println!("{telegram}");
    println!("{token}");
}
