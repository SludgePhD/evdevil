# `evdevil`: Rust bindings to *evdev* and *uinput*

This library provides an interface to the Linux and FreeBSD input device subsystem.

## Features

- Ergonomic Rusty API designed for stability.
- Exposes almost every `evdev` and `uinput` feature, including force-feedback and multitouch.
- Device Hotplug support.
- Light on dependencies.
- FreeBSD compatibility.
- `async` support via Tokio or `async-io`.

## Cargo Features

|  Feature   | Description |
|------------|-------------|
| `serde`    | Implements `Serialize` and `Deserialize` for many event code wrappers like `Key`, `Rel`, etc. |
| `tokio`    | Uses tokio to enable `async` event reading using `EventReader`. |
| `async-io` | Uses `async-io` to enable `async` event reading using `EventReader`. |

## Rust Support

This library targets the latest Rust version.

Older Rust versions are supported by equally older versions of this crate. For example, to use a
version of Rust that was succeeded 6 months ago, you'd also use an at least 6 month old version of
this library.

Compatibility with older Rust versions may be provided on a best-effort basis.

## Development

### Testing

The crate is tested using end-to-end tests that create a virtual `uinput` device and then open it.
This means the user running the tests needs to have permission to write to `/dev/uinput` and the input devices in `/dev/input/event*`.
