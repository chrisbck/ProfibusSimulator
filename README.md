# profibus-dp-sim

PROFIBUS-DP Simulator (byte-stream generator) — learning-focused Rust project.

Purpose
- Provide a deterministic, configurable byte-stream simulator of PROFIBUS-DP telegrams to aid development of a separate ESP32-based passive PROFIBUS analyzer.

Relation to ESP32 analyzer
- The simulator will generate raw PROFIBUS byte streams so the ESP32 parser and diagnostic logic can be developed and tested without a live PROFIBUS network.

Current status
- Initial project skeleton. No simulator functionality implemented yet.

Getting started
```sh
# build
cargo build

# run (prints a small banner)
cargo run

# run tests
cargo test
```

Roadmap (high level)
- Represent simple byte streams
- Model PROFIBUS telegrams and delimiters
- Implement FCS calculation and verification
- Serialize frames and stream sequences
- Model master/slave exchanges and fault injection

License
- Public domain for learning and experimentation.
