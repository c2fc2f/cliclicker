//! A fast Wayland autoclicker

use anyhow::Context;
use clap::Parser;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, InputEvent, KeyCode};
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
#[derive(Parser, Debug)]
#[command(author, version, long_about = None)]
struct Args {
    /// Path to the physical device event file (e.g., /dev/input/by-id/usb...)
    #[arg(short, long)]
    device: PathBuf,

    /// Button used to trigger the autoclicker (e.g., BTN_SIDE)
    /// See https://docs.rs/evdev/latest/evdev/struct.KeyCode.html
    #[arg(long, default_value = "BTN_SIDE")]
    trigger: KeyCode,

    /// Target button to rapidly click (e.g., BTN_LEFT)
    /// See https://docs.rs/evdev/latest/evdev/struct.KeyCode.html
    #[arg(long, default_value = "BTN_LEFT")]
    target: KeyCode,

    /// Clicks per second target rate
    #[arg(short, long, default_value_t = 20)]
    cps: u64,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut physical_mouse =
        Device::open(&args.device).context("Failed to open physical device")?;

    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(args.target);
    let mut virtual_mouse: VirtualDevice = VirtualDevice::builder()?
        .name("Rust Fast Autoclicker")
        .with_keys(&keys)?
        .build()?;

    let is_clicking = Arc::new(AtomicBool::new(false));
    let wakeup = Arc::new(Condvar::new());

    println!("Device  : {:?}", args.device);
    println!("Trigger : {:?}", args.trigger);
    println!("Target  : {:?} @ {} cps", args.target, args.cps);

    thread::spawn({
        let is_clicking = Arc::clone(&is_clicking);
        let wakeup = Arc::clone(&wakeup);

        move || {
            loop {
                match physical_mouse.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() != EventType::KEY
                                || ev.code() != args.trigger.code()
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

    let half_delay = Duration::from_secs_f64(0.5 / args.cps as f64);
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
                args.target.code(),
                1,
            )])?;
            thread::sleep(half_delay);
            virtual_mouse.emit(&[InputEvent::new(
                EventType::KEY.0,
                args.target.code(),
                0,
            )])?;
            thread::sleep(half_delay);
        }
    }
}

