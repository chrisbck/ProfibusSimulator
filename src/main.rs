#[derive(Debug)]
struct Telegram {
    name: String,
    bytes: Vec<u8>,
}

impl Telegram {
    fn new(name: &str, bytes: Vec<u8>) -> Self {
        Self {
            name: name.to_string(),
            bytes,
        }
    }

    fn print(&self) {
        print!("{}: ", self.name);

        for byte in &self.bytes {
            print!("{byte:02X} ");
        }

        println!();
    }
}

fn main() {
    let telegram = Telegram::new("Test telegram", vec![0x10, 0x05, 0x02, 0x49, 0x50, 0x16]);

    telegram.print();
}
