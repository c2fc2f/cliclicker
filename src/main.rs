//! A fast Wayland autoclicker

use anyhow::Context;
use clap::Parser;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::rng;
use rand_distr::{Beta, Distribution, LogNormal, Uniform};
use serde::Deserialize;
use std::fs::read_to_string;
use std::ops::Range;
use std::time::SystemTime;
use std::{
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// A fast Wayland autoclicker
#[derive(clap::Parser, Debug)]
#[command(author, version, long_about = None)]
struct Args {
    /// Configuration from a TOML file
    #[arg(long)]
    config: PathBuf,
}

/// Autoclicker configuration
#[derive(Deserialize, Debug)]
struct Config {
    /// Display name for the virtual input device created by the autoclicker
    #[serde(default = "default_name")]
    name: String,

    /// Path to the physical device event file (e.g., /dev/input/by-id/usb...)
    device: PathBuf,

    /// Button used to trigger the autoclicker (e.g., BTN_SIDE)
    /// See https://docs.rs/evdev/latest/evdev/struct.KeyCode.html
    #[serde(default = "default_trigger")]
    trigger: KeyCode,

    /// Target button to rapidly click (e.g., BTN_LEFT)
    /// See https://docs.rs/evdev/latest/evdev/struct.KeyCode.html
    #[serde(default = "default_target")]
    target: KeyCode,

    /// Clicks per second target rate
    #[serde(default = "default_cps")]
    cps: u64,

    /// A list of randomization layers applied to the base interval between
    /// clicks
    #[serde(default)]
    random: Vec<RandomDelayConfig>,
}

/// Default name of the input
fn default_name() -> String {
    "Rust Fast Autoclicker".to_owned()
}

/// Default trigger
fn default_trigger() -> KeyCode {
    KeyCode::BTN_SIDE
}

/// Default trigger
fn default_target() -> KeyCode {
    KeyCode::BTN_LEFT
}

/// Default cps
fn default_cps() -> u64 {
    20
}

/// Configuration for randomized intervals to prevent detection or simulate
/// human behavior
#[derive(Deserialize, Debug, Default)]
struct RandomDelayConfig {
    /// Random delay variance (min..max) added to the interval between clicks
    delay: Range<u64>,

    /// The statistical distribution model applied to the random delay
    #[serde(default)]
    distribution: DistType,
}

/// Specifies the statistical distribution or noise algorithm used to generate
/// randomized timing or delays
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum DistType {
    /// A flat distribution where every possible value within the target range
    /// has an identical chance of occurring. This is the default baseline
    /// behavior
    #[default]
    Uniform,

    /// A Beta distribution configured to create U-shaped or bell-shaped
    /// curves. Useful for clustering generated values toward the extremes or
    /// the center of a range
    UShape {
        /// The alpha shape parameter controlling the distribution curve
        alpha: f64,
        /// The beta shape parameter controlling the distribution curve
        beta: f64,
    },

    /// A log-normal distribution. This is highly effective for simulating
    /// human reaction times, which tend to have a strict minimum bound but a
    /// long "tail" of slower outliers
    LogNormal {
        /// The mean of the underlying normal distribution.
        /// Typically kept low (e.g., 0.0 to 1.0) to generate subtle offsets
        mean: f64,
        /// The standard deviation of the underlying normal distribution.
        /// Dictates the spread and severity of the right-skewed tail
        /// (e.g., 0.5 to 1.0)
        std_dev: f64,
    },

    /// Generates smooth, continuous pseudorandom values using 1D Perlin
    /// noise. Ideal for simulating natural, organic timing drifts rather than
    /// erratic jumps
    Perlin {
        /// The frequency of the noise wave. Lower values result in smoother,
        /// more gradual transitions between delays over time
        frequency: f64,
    },

    /// Fractional Brownian Motion (fBm). Builds upon basic noise by stacking
    /// multiple frequencies and amplitudes to create complex, textured timing
    /// patterns
    Fbm {
        /// The base frequency of the initial noise layer (e.g., 0.5)
        frequency: f64,
        /// The number of noise layers (octaves) stacked together.
        /// Values between 2 and 5 are typical, with 3 offering a solid
        /// balance of detail and realism
        octaves: usize,
    },

    /// A probabilistic trigger designed to simulate sudden human errors,
    /// distractions, or micro-pauses. It generally adds no delay, but
    /// occasionally introduces a significant lag spike
    Outlier {
        /// The probability threshold (from 0.0 to 1.0) of an outlier
        /// occurring. For example, `0.03` represents a 3% chance of
        /// triggering a major delay per action
        probability: f64,
    },
}

impl RandomDelayConfig {
    /// Generates a random delay based on the deserialized configuration
    pub fn sample(&self) -> u64 {
        let mut rng = rng();

        let min = self.delay.start as f64;
        let max = self.delay.end as f64;

        if min >= max {
            return 0;
        }

        let sample_f64 = match self.distribution {
            DistType::Uniform => {
                let dist = Uniform::new(min, max).unwrap();
                dist.sample(&mut rng)
            }
            DistType::UShape { alpha, beta } => {
                let dist = Beta::new(alpha, beta).unwrap();
                let normalized = dist.sample(&mut rng);

                min + (max - min) * normalized
            }
            DistType::LogNormal { mean, std_dev } => {
                let dist = LogNormal::new(mean, std_dev).unwrap();
                let offset = dist.sample(&mut rng);

                (min + offset).clamp(min, max)
            }
            DistType::Perlin { frequency } => {
                let perlin = Perlin::new(1);

                let time_x = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64();
                let noise_val = perlin.get([time_x * frequency, 0.0]);
                let normalized = ((noise_val + 1.0) / 2.0).clamp(0.0, 1.0);

                min + (max - min) * normalized
            }
            DistType::Fbm { frequency, octaves } => {
                let fbm = Fbm::<Perlin>::new(1).set_octaves(octaves);

                let time_x = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs_f64();

                let noise_val = fbm.get([time_x * frequency, 0.0]);
                let normalized = ((noise_val + 1.0) / 2.0).clamp(0.0, 1.0);

                min + (max - min) * normalized
            }

            DistType::Outlier { probability } => {
                let prob_dist = Uniform::new(0.0, 1.0).unwrap();
                let roll = prob_dist.sample(&mut rng);

                if roll <= probability {
                    let delay_dist = Uniform::new(min, max).unwrap();
                    delay_dist.sample(&mut rng)
                } else {
                    0.0
                }
            }
        };

        sample_f64.round() as u64
    }
}

fn main() -> anyhow::Result<()> {
    let args =
        Args::try_parse().context("Failed to parse command-line arguments")?;
    let config: Config =
        toml::from_str(&read_to_string(&args.config).with_context(|| {
            format!("Failed to read config file: {}", args.config.display())
        })?)
        .with_context(|| {
            format!(
                "Failed to parse config file as TOML: {}",
                args.config.display()
            )
        })?;

    let mut physical_mouse = Device::open(&config.device)
        .context("Failed to open physical device")?;

    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(config.target);
    let mut virtual_mouse: VirtualDevice = VirtualDevice::builder()?
        .name(&config.name)
        .with_keys(&keys)?
        .build()?;

    let is_clicking = Arc::new(AtomicBool::new(false));
    let wakeup = Arc::new(Condvar::new());

    println!("Device  : {:?}", config.device);
    println!("Trigger : {:?}", config.trigger);
    println!("Target  : {:?} @ {} cps", config.target, config.cps);

    thread::spawn({
        let is_clicking = Arc::clone(&is_clicking);
        let wakeup = Arc::clone(&wakeup);

        move || {
            loop {
                match physical_mouse.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() != EventType::KEY
                                || ev.code() != config.trigger.code()
                            {
                                continue;
                            }

                            match ev.value() {
                                1 => {
                                    is_clicking.store(true, Ordering::Relaxed);
                                    println!("Trigger pressed  → clicking...");
                                    wakeup.notify_one();
                                }
                                0 => {
                                    is_clicking.store(false, Ordering::Relaxed);
                                    println!("Trigger released → stopped.");
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Fatal: error reading events: {e}");
                        is_clicking.store(false, Ordering::Relaxed);
                        wakeup.notify_one();
                        break;
                    }
                }
            }
        }
    });

    let half_delay = Duration::from_secs_f64(0.5 / config.cps as f64);
    let mutex = Mutex::new(());

    loop {
        {
            let guard = mutex.lock().expect("Poisoned mutex");
            let _guard = wakeup
                .wait_while(guard, |_| !is_clicking.load(Ordering::Relaxed))
                .expect("Poisoned mutex");
        }

        while is_clicking.load(Ordering::Relaxed) {
            virtual_mouse.emit(&[InputEvent::new(
                EventType::KEY.0,
                config.target.code(),
                1,
            )])?;
            thread::sleep(
                half_delay
                    + Duration::from_millis(
                        config
                            .random
                            .iter()
                            .map(RandomDelayConfig::sample)
                            .sum(),
                    ),
            );
            virtual_mouse.emit(&[InputEvent::new(
                EventType::KEY.0,
                config.target.code(),
                0,
            )])?;
            thread::sleep(half_delay);
        }
    }
}
