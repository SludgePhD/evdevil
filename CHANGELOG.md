# Changelog

## v0.1.1

- Renamed `Evdev::can_read` and `UinputDevice::can_read` to `Evdev::is_readable`
  and `UinputDevice::is_readable`, respectively (with `can_read` becoming a
  deprecated alias).
- Added `Evdev::block_until_readable` and `UinputDevice::block_until_readable`.

## v0.1.0

Initial release.
