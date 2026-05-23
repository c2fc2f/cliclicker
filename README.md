# cliclicker

A fast Wayland autoclicker written in Rust. It listens to a physical input device via `evdev`, toggles a clicking loop when a configurable trigger button is held, and emits synthetic input events through a `uinput` virtual device — bypassing the compositor entirely.

## Overview

Most autoclickers rely on X11 tooling (xdotool, xte) that does not work under Wayland. cliclicker operates at the kernel input layer instead: it reads raw events from a physical device file and injects synthetic button presses through a virtual `uinput` device, which the compositor sees as a real mouse. No display server protocol is involved.

The clicking thread parks itself on a `Condvar` when idle and is woken immediately when the trigger is pressed, keeping CPU usage at zero between bursts. Click timing is split symmetrically: half the interval is spent in the pressed state, half in the released state, which produces events that applications recognize as genuine clicks.

## Requirements

- Linux kernel with `uinput` support (`CONFIG_INPUT_UINPUT`)
- Wayland compositor (X11 is not tested)
- Read access to the target device file (typically requires membership in the `input` group)
- Write access to `/dev/uinput`
- Rust toolchain (edition 2024, stable) — or Nix with flakes enabled

## Installation

### From source

```bash
git clone https://github.com/c2fc2f/cliclicker
cd cliclicker
cargo build --release
```

The compiled binary will be at `target/release/cliclicker`.

### With Nix

A Nix flake is provided:

```bash
nix run github:c2fc2f/cliclicker -- --help
# or
nix build
# or, to enter a development shell:
nix develop
```

## Permissions

The process needs read access to the physical device and write access to `/dev/uinput`. The cleanest way to grant both without running as root is to add your user to the `input` group and set up a udev rule:

```bash
sudo usermod -aG input $USER
```

```udev
# /etc/udev/rules.d/99-uinput.rules
KERNEL=="uinput", GROUP="input", MODE="0660"
```

On NixOS:

```nix
users.users.<name>.extraGroups = [ "input" ];
services.udev.extraRules = ''
  KERNEL=="uinput", GROUP="input", MODE="0660"
'';
```

## Usage

```
cliclicker --device <PATH> [OPTIONS]
```

| Flag | Short | Description | Default |
|---|---|---|---|
| `--device <PATH>` | `-d` | Path to the physical device event file | *(required)* |
| `--trigger <KEY>` | | Button that activates the autoclicker while held | `BTN_SIDE` |
| `--target <KEY>` | | Button to emit rapidly | `BTN_LEFT` |
| `--cps <N>` | `-c` | Target click rate in clicks per second | `20` |

Key names follow the `evdev` naming convention. A full list is available at [docs.rs/evdev](https://docs.rs/evdev/latest/evdev/struct.KeyCode.html).

To find the path of your mouse:

```bash
ls /dev/input/by-id/
```

### Examples

Click left mouse button at 20 cps while the side button is held, using a mouse identified by its USB id:

```bash
cliclicker --device /dev/input/by-id/usb-Logitech_USB_Receiver-if02-event-mouse
```

Use the extra thumb button as the trigger and click at 30 cps:

```bash
cliclicker --device /dev/input/by-id/... --trigger BTN_EXTRA --cps 30
```

Click the right button instead:

```bash
cliclicker --device /dev/input/by-id/... --target BTN_RIGHT
```

## License

This project is licensed under the [MIT License](LICENSE).
