# Vox Remote Desktop

A high-performance remote desktop application built with Rust and egui, featuring code-based authentication and encrypted communication.

## Features

- **Cross-platform**: Works on macOS, Windows, and Linux
- **Code-based authentication**: Secure 6-digit access codes that expire after 5 minutes
- **Encrypted communication**: All data is encrypted using AES-256-GCM with X25519 key exchange
- **High performance**: Uses efficient screen capture and compression
- **Native GUI**: Built with egui for a responsive interface

## Architecture

The application consists of two main components:

1. **Server** (`vox_server`): Runs on the machine to be accessed remotely
   - Generates access codes for authentication
   - Captures screen content
   - Handles input events (mouse/keyboard)
   
2. **Client** (`vox_client`): Connects to remote machines
   - GUI interface for entering access codes
   - Displays remote screen
   - Forwards input events

## Building

```bash
# Build both server and client
cargo build --release

# Build only the server
cargo build --release --bin vox_server

# Build only the client
cargo build --release --bin vox_client
```

## Usage

### Running the Server

```bash
cargo run --bin vox_server
```

The server will display a 6-digit access code that expires in 5 minutes:

```
=================================
Access Code: 123456
Code expires in 5 minutes
=================================
```

### Running the Client

```bash
cargo run --bin vox_client
```

1. Enter the 6-digit access code displayed by the server
2. Optionally change the server address (default: 127.0.0.1:8080)
3. Click "Connect"

## Security

- Access codes are randomly generated and expire after 5 minutes
- All communication is encrypted using AES-256-GCM
- Key exchange uses X25519 Diffie-Hellman
- Passwords are hashed using Argon2

## Development Status

This is a foundational implementation. The following features are planned:

- [ ] Full network implementation (currently uses placeholders)
- [ ] Video codec integration (VP8/VP9 or H.264)
- [ ] Audio streaming
- [ ] File transfer
- [ ] Multiple monitor support
- [ ] Clipboard synchronization
- [ ] Session recording

## Dependencies

Key dependencies include:
- `egui` & `eframe`: GUI framework
- `scrap`: Screen capture
- `enigo`: Input simulation
- `quinn`: QUIC networking (planned)
- `rustls`: TLS encryption
- `argon2`: Password hashing
- `aes-gcm` & `x25519-dalek`: Encryption