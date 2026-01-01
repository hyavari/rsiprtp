# mdsiprtp & Gabby - SIP/RTP Stack and Voice AI Agent

## Project Overview

This workspace contains **mdsiprtp**, a production-ready, modular SIP/RTP communications stack written in Rust, and **Gabby**, a Voice AI agent application built on top of it.

### Key Components

*   **mdsiprtp**: A comprehensive SIP/RTP stack designed for voicemail applications and AI agent call bridges. It follows a layered architecture:
    *   `mdsiprtp-core`: Common types and configuration.
    *   `mdsiprtp-sip`: SIP message parsing and building (wraps `rsip`).
    *   `mdsiprtp-transaction`: RFC 3261 transaction state machines (Sans-IO pattern).
    *   `mdsiprtp-dialog`: INVITE dialog management.
    *   `mdsiprtp-session`: High-level call management.
    *   `mdsiprtp-rtp` / `mdsiprtp-media`: RTP packet handling and audio codecs (G.711).
*   **Gabby** (`crates/gabby`): A standalone Voice AI agent that accepts SIP calls and converses using:
    *   **Vosk**: Offline Speech-to-Text (STT).
    *   **Ollama**: Local LLM inference (llama3.2).
    *   **Piper**: Neural Text-to-Speech (TTS).

## Getting Started

### Prerequisites

*   **Rust**: 1.70+
*   **Docker**: For running integration tests (Asterisk).
*   **Gabby Specifics**: Linux (x86_64/aarch64), ~2GB disk space, 4GB+ RAM.

### Building

```bash
cargo build --workspace
```

### Running Gabby (Voice AI Agent)

1.  **Install Dependencies**:
    Use the setup script to download Vosk models and Piper:
    ```bash
    cd crates/gabby
    ./scripts/setup.sh
    ```

2.  **Start Ollama**:
    In a separate terminal:
    ```bash
    ollama serve
    # Ensure model is available: ollama pull llama3.2:3b
    ```

3.  **Run**:
    ```bash
    cargo run --release -p gabby
    ```
    Gabby will listen on `0.0.0.0:5060` (SIP) and `10000-20000` (RTP).

### Integration Testing (Infrastructure)

The `docker/` directory contains a Docker Compose setup for running an Asterisk server to test SIP integration.

```bash
# Start Asterisk
docker compose -f docker/docker-compose.yml up -d

# Run integration tests
cargo test --test integration_*
```

## Development Workflow

*   **Architecture**: The project uses a **Sans-IO** pattern for state machines (transactions, dialogs), making core logic deterministic and easy to test without network mocking.
*   **Testing**: High test coverage is maintained.
    *   Run unit tests: `cargo test`
    *   Run specific crate tests: `cargo test -p mdsiprtp-transaction`
*   **Code Quality**: The codebase is rated "Good" in recent reviews, with clean separation of concerns.
*   **Known Issues (Phase 1)**:
    *   PRNG is time-based (needs migration to cryptographic random for production).
    *   RTCP is not yet implemented.
    *   TLS/SRTP support is planned for Phase 2.

## Directory Structure

*   `crates/`: Source code for all workspace members.
    *   `gabby/`: The Voice AI agent application.
    *   `mdsiprtp*/`: The modular SIP/RTP stack components.
*   `docker/`: Infrastructure configuration (Asterisk) for testing.
*   `wrk_docs/`: Project documentation, code reviews, and architectural notes.
*   `wrk_journals/`: Development logs.

## Key Commands

| Action | Command |
| :--- | :--- |
| **Build Workspace** | `cargo build --workspace` |
| **Test All** | `cargo test --workspace` |
| **Run Gabby** | `cargo run -p gabby` |
| **Format Code** | `cargo fmt` |
| **Lint Code** | `cargo clippy` |
| **Start Infrastructure** | `docker compose -f docker/docker-compose.yml up -d` |
