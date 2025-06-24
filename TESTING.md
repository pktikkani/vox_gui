# Vox Remote Desktop Testing Guide

## Quick Start

### On Intel Mac (Server)
1. Open terminal and navigate to the project directory
2. Run the server:
   ```bash
   cargo run --bin vox_server
   ```
3. Note the 6-digit access code displayed
4. Note your IP address:
   ```bash
   ifconfig | grep "inet " | grep -v 127.0.0.1
   ```

### On Apple Silicon Mac (Client)
1. Open terminal and navigate to the project directory
2. Run the client:
   ```bash
   cargo run --bin vox_client
   ```
3. In the GUI:
   - Enter the 6-digit code from the server
   - Replace `127.0.0.1:8080` with `[INTEL_MAC_IP]:8080`
   - Click Connect

## What Should Happen

1. **Server Side**:
   - Shows "Server listening on 0.0.0.0:8080"
   - Displays access code (e.g., "Access code: 123456")
   - Shows "New connection from..." when client connects
   - Shows "Authentication successful" after correct code

2. **Client Side**:
   - GUI window opens with connection form
   - After entering code and IP, clicking Connect should:
     - Show "Connecting..." briefly
     - Switch to remote desktop view
     - Display the Intel Mac's screen
     - Allow mouse/keyboard control

## Features

- **Security**: 6-digit access codes (5-minute expiry)
- **Encryption**: AES-256-GCM with X25519 key exchange
- **Compression**: Zstd compression for screen data
- **Performance**: 30 FPS screen capture
- **Cross-platform**: Works on macOS, Windows, Linux

## Troubleshooting

1. **Connection Failed**:
   - Check firewall settings
   - Ensure both Macs are on same network
   - Verify correct IP address
   - Try port 8080 is not blocked

2. **Black Screen**:
   - macOS may require screen recording permission
   - Go to System Preferences → Security & Privacy → Screen Recording
   - Add Terminal or your IDE

3. **Input Not Working**:
   - macOS requires accessibility permissions
   - Go to System Preferences → Security & Privacy → Accessibility
   - Add Terminal or your IDE

## Debug Mode

For more verbose output:
```bash
RUST_LOG=debug cargo run --bin vox_server
RUST_LOG=debug cargo run --bin vox_client
```