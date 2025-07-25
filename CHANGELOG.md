# Changelog

## v0.3.2

- Try to fix the docs.rs render.

## v0.3.1

- Try to fix the docs.rs render.

## v0.3.0

### Breaking Changes

- `HotplugMonitor` no longer implements `Iterator`, but now implements `IntoIterator`.
- `HotplugMonitor` now yields `HotplugEvent`s instead of already-opened `Evdev`s.
  - Call `HotplugEvent::open` to open the device.
- `hotplug::enumerate` has moved to `enumerate_hotplug` in the crate root.

### New Features

#### Async

`EventReader` now allows reading events and reports via `async`.
This functionality requires enabling either the `"tokio"` or `"async-io"` Cargo features.

Note that a lot of evdev functionality cannot be made `async` and will always block.
Only reading events asynchronously via the `EventReader` is supported for now.

### Other Changes

- FreeBSD: sleep after connecting to `devd` to ensure no events go missing.
- Mark some methods `#[inline]` for more efficient code generation.
- Don't redundantly invoke `fcntl` if the non-blocking status is already what we want.
- Implement `AsFd` and `IntoRawFd` for `HotplugMonitor`.
- Include device path in error message if opening fails.
- Add `Report::len`, returning the number of events in the `Report`.
- Device enumeration iterators have been made real types instead of `impl Iterator` and moved to
  the `enumerate` module.

## v0.2.3

- Update from `linux-ioctl` to `uoctl` 1.0.

## v0.2.2

- Support FreeBSD, and test it in CI.
- Improve error messages when some `ioctl`s fail.

## v0.2.1

### Fixes

- Fix `InvalidInput` error when creating `EventReader`s for most devices.

## v0.2.0

### Breaking Changes

- Remove previously deprecated items.
- Change `Evdev::path` to return `&Path` instead of `Option<&Path>`.
- Replace `Iterator` impl of `Report` with `IntoIterator` impls.
- Change `InputEvent::kind` to return `EventKind` instead of `Option<EventKind>`.

### New Features

- Implement `AsFd`, `AsRawFd`, and `IntoRawFd` for `EventReader` to mirror `Evdev`.
- Make most `AbsInfo` methods `const`.

## v0.1.4

- Add `EventReader::reports` for iterating over `Report`s.
- Deprecate `EventReader::next_report` in favor of `EventReader::reports`.
- Generate synthetic multitouch events when `EventReader` is created or events are dropped.
- Fall back to write-only mode if opening an `Evdev` in read-only mode fails due to lack of permission.

## v0.1.3

- Add `EventReader::next_report` for fetching whole `Report`s from the device rather than events.

## v0.1.2

- Add `serde` feature that implements `Serialize` and `Deserialize` for some of the event code
  wrapper types like `Key`, `Rel`, `Abs`, etc.

## v0.1.1

- Renamed `Evdev::can_read` and `UinputDevice::can_read` to `Evdev::is_readable`
  and `UinputDevice::is_readable`, respectively (with `can_read` becoming a
  deprecated alias).
- Added `Evdev::block_until_readable` and `UinputDevice::block_until_readable`.

## v0.1.0

Initial release.
