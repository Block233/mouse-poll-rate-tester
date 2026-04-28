# Mouse Poll Rate Tester

A lightweight Windows tool that measures and displays your mouse's polling rate in real time using the Windows Raw Input API.

## Features

- Real-time polling rate display (Hz)
- Sliding 1-second window calculation
- Average, maximum, and minimum rate tracking
- Visual bar indicator with color coding
- HiDPI (per-monitor DPI v2) support
- GDI double buffering for flicker-free rendering
- Pure Win32 API — no frameworks, no runtime dependencies
- No console window (Windows subsystem)

## System Requirements

- Windows 10 or later
- A mouse (wired or wireless)

## Download

Pre-built binaries are available on the [Releases](https://github.com/akiflax/mouse-poll-rate-tester/releases) page.

## Build from Source

```sh
git clone https://github.com/akiflax/mouse-poll-rate-tester.git
cd mouse-poll-rate-tester
cargo build --release
```

The binary will be at `target/release/mouse_poll_rate_tester.exe`.

Requires Rust **1.85+** (edition 2024).

## How It Works

The program registers for raw mouse input via `RegisterRawInputDevices` with the `RIDEV_INPUTSINK` flag, which captures all mouse movement even when the window is not focused (though the window must remain open).

Each `WM_INPUT` message triggers a timestamp capture using `QueryPerformanceCounter`. A sliding 1-second window of timestamps is maintained to compute the instantaneous polling rate.

## License

MIT
