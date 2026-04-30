# rsiprtp-sip fuzzing

Coverage-guided fuzzing of the SIP message parser
(`rsiprtp_sip::SipMessage::parse`) using
[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) and `libFuzzer`.

SIP messages arrive over UDP/TCP from untrusted peers, so the parser is a
high-value attack surface. This harness exercises it with arbitrary bytes and
asserts only that parsing must not panic — `Err` is the expected outcome on
malformed input.

## Prerequisites

- A nightly Rust toolchain (libFuzzer is nightly-only):
  `rustup toolchain install nightly`
- `cargo-fuzz`: `cargo install cargo-fuzz --locked`
- libFuzzer ships with `rustc`'s `compiler-rt` on Linux and macOS. On Windows,
  libFuzzer support is limited; prefer running fuzzing from WSL or a Linux/macOS
  host.

## Run

From the crate root (`crates/rsiprtp-sip/`):

```bash
# Build only (sanity check that the harness compiles):
cargo +nightly fuzz build

# Run the parser fuzzer indefinitely (Ctrl-C to stop):
cargo +nightly fuzz run parse_sip_message

# Time-bounded run (e.g. 5 minutes):
cargo +nightly fuzz run parse_sip_message -- -max_total_time=300

# Reproduce a crash from a saved artifact:
cargo +nightly fuzz run parse_sip_message fuzz/artifacts/parse_sip_message/crash-<hash>
```

Crashing inputs are written to `fuzz/artifacts/parse_sip_message/`. New
interesting inputs discovered by the fuzzer are added to
`fuzz/corpus/parse_sip_message/`.

## Seed corpus

`fuzz/corpus/parse_sip_message/` ships with hand-written, RFC 3261–valid SIP
messages (INVITE, 200 OK, BYE, REGISTER, 401 challenge, 100 Trying, CANCEL,
ACK, OPTIONS, 486 Busy Here). They use CRLF line endings as required by SIP.

To add more seeds, drop raw byte files into
`fuzz/corpus/parse_sip_message/`. Real-world captures (e.g. extracted from
pcaps via `tshark -Y sip -T fields -e sip`) make excellent seeds. Keep each
seed to a single SIP message; libFuzzer prefers small inputs.

## Notes

- This crate is intentionally not a workspace member; `cargo-fuzz` requires its
  own isolated build with sanitizer flags. Run all `cargo fuzz` commands from
  the `crates/rsiprtp-sip/` directory.
- Fuzzing is not wired into CI. Run it locally before releases or after
  changes to `message.rs` / `headers.rs` / the underlying `rsip` dependency.
