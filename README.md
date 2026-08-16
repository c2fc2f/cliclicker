# cliclicker

A fast Wayland autoclicker written in Rust. It listens to a physical input device via `evdev`, toggles a clicking loop when a configurable trigger button is held, and emits synthetic input events through a `uinput` virtual device — bypassing the compositor entirely.

## Overview

Most autoclickers rely on X11 tooling (xdotool, xte) that does not work under Wayland. cliclicker operates at the kernel input layer instead: it reads raw events from a physical device file and injects synthetic button presses through a virtual `uinput` device, which the compositor sees as a real mouse. No display server protocol is involved.

The clicking thread parks itself on a `Condvar` when idle and is woken immediately when the trigger is pressed, keeping CPU usage at zero between bursts. Click timing splits the interval in half: half is spent in the pressed state, half in the released state, producing events that applications recognize as genuine clicks. An optional stack of randomization layers adds extra jitter on top of the pressed half, so repeated clicks do not land with mechanical precision — see [Randomization](#randomization).

The project is a single Cargo package (no workspace). The `cliclicker` binary is the sole deliverable, built on:

- **`evdev`** — reads raw events from the physical device and emits synthetic events through the virtual `uinput` device
- **`clap`** — command-line argument parsing
- **`serde`** / **`toml`** — configuration file parsing
- **`rand`**, **`rand_distr`**, **`noise`** — sampling for the randomization layers (uniform, Beta, log-normal, and Perlin/fBm noise)

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
cliclicker --config <PATH>
```

| Flag | Description | Default |
|---|---|---|
| `--config <PATH>` | Path to the TOML configuration file — see [Configuration](#configuration) | *(required)* |

Device, trigger, target, click rate, and timing randomization are all set in the config file; there are no other command-line flags.

### Examples

Run with a config file:

```bash
cliclicker --config config.toml
```

To find the path of your mouse:

```bash
ls /dev/input/by-id/
```

## Configuration

The file passed to `--config` is parsed as TOML into the following structure.

### Top-level fields

| Field | Description | Default |
|---|---|---|
| `name` | Display name for the virtual `uinput` device | `"Rust Fast Autoclicker"` |
| `device` | Path to the physical device event file. Can be a mouse or a keyboard — whichever produces the `trigger` events | *(required)* |
| `trigger` | Key or button that activates the autoclicker while held | `BTN_SIDE` |
| `target` | Button to emit rapidly while triggered | `BTN_LEFT` |
| `cps` | Target click rate, in clicks per second | `20` |
| `random` | List of randomization layers stacked on top of the base interval — see [Randomization](#randomization) | `[]` |

`trigger` and `target` follow the `evdev` naming convention (`BTN_SIDE`, `KEY_F8`, …); a full list is available at [docs.rs/evdev](https://docs.rs/evdev/latest/evdev/struct.KeyCode.html).

### Randomization

Each entry in `random` adds extra delay, in milliseconds, on top of the pressed half of the click cycle; when several entries are present, their sampled delays are summed. The released half of the cycle is never randomized.

| Field | Description | Default |
|---|---|---|
| `delay` | Delay range, in milliseconds, written as `{ start = <N>, end = <N> }` | *(required)* |
| `distribution` | The statistical distribution or noise algorithm used to sample a value from `delay` | `uniform` |

If `start >= end` for an entry, that layer contributes no delay.

`distribution.type` selects one of:

- **`uniform`** *(default)* — flat distribution: every value in `delay` is equally likely.
- **`u_shape`** — Beta distribution with shape parameters `alpha` and `beta`; clusters sampled values toward the extremes or the center of the range.
- **`log_normal`** — log-normal distribution with `mean` and `std_dev`; good for simulating human reaction times, which tend to have a strict minimum bound but a long tail of slower outliers.
- **`perlin`** — smooth, continuous pseudorandom values from 1D Perlin noise, driven by `frequency` (lower values give smoother, more gradual drift over time).
- **`fbm`** — fractional Brownian motion: `frequency` for the base octave plus `octaves` (2–5 typical, 3 offering a solid balance of detail and realism) stacked together for more textured drift.
- **`outlier`** — adds no delay most of the time; with probability `probability` (`0.0`–`1.0`, e.g. `0.03` for a 3% chance) it instead samples a full delay from `delay`, simulating a sudden human error, distraction, or micro-pause.

### Sample configuration

```toml
name = "Gaming chair"
device = "/dev/input/by-id/usb-Logitech_USB_Receiver-if02-event-mouse"
trigger = "KEY_F8"
target = "BTN_LEFT"
cps = 15

[[random]]
delay = { start = 0, end = 25 }
[random.distribution]
type = "log_normal"
mean = 0.5
std_dev = 1.0

[[random]]
delay = { start = 0, end = 20 }
[random.distribution]
type = "fbm"
frequency = 0.5
octaves = 4

[[random]]
delay = { start = 30, end = 85 }
[random.distribution]
type = "outlier"
probability = 0.05
```

This example uses `KEY_F8` (a keyboard key) as the trigger instead of a mouse button, and stacks three randomization layers: a log-normal jitter, an fBm drift, and an occasional 5%-chance outlier pause. See [`examples/config.toml`](examples/config.toml).

## License

This project is licensed under the [MIT License](LICENSE).
