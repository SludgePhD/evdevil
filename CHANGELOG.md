# Changelog

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
