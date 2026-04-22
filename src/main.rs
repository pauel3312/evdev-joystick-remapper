use evdev::{Device, InputEvent, EventType, AbsoluteAxisCode, AbsInfo, UinputAbsSetup};
use evdev::uinput::VirtualDevice;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

fn linear_map(
    val: i32,
    in_min: i32,
    in_max: i32,
    out_min: i32,
    out_max: i32,
    invert: bool,
) -> i32 {
    let in_range = in_max - in_min;
    let mut adj_val = val - in_min;
    let out_range = out_max - out_min;

    if invert {
        adj_val = in_range - adj_val;
    }

    let out_val_adj =
        ((out_range as f32 / in_range as f32) * adj_val as f32) as i32;

    let rt = out_val_adj + out_min;

    rt.clamp(out_min, out_max)
}

type MapFn = fn(i32) -> i32;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- Find devices ---
    let mut ursa_minor = None;
    let mut twcs = None;

    for (path, dev) in evdev::enumerate() {
        let name = dev.name().unwrap_or("Unknown");
        println!("{} -> {}", path.display(), name);

        if name == "Winwing WINCTRL URSA MINOR Combat Joystick R" {
            ursa_minor = Some(dev);
        } else if name == "Thrustmaster TWCS Throttle" {
            twcs = Some(dev);
        }
    }

    let ursa_minor = ursa_minor.expect("URSA MINOR not found");
    let twcs = twcs.expect("TWCS not found");

    // --- Create virtual device ---
    let vdev = VirtualDevice::builder()?
        .name("Combined Virtual Device")
        .with_absolute_axis(
            &UinputAbsSetup::new(
                AbsoluteAxisCode::ABS_THROTTLE,
                AbsInfo::new(0, -32767, 32767, 0, 0, 0)
            )
        )?
        .with_absolute_axis(
            &UinputAbsSetup::new(
                AbsoluteAxisCode::ABS_BRAKE,
                AbsInfo::new(0, -32767, 32767, 0, 0, 0),
            )
        )?
        .build()?;

    println!("Virtual device created");
    println!("Starting input merger… Press Ctrl+C to exit.");

    let vdev = Arc::new(Mutex::new(vdev));

    // --- Axis mapping ---
    let mut axis_map: HashMap<(u8, AbsoluteAxisCode), (AbsoluteAxisCode, MapFn)> =
        HashMap::new();

    axis_map.insert(
        (2, AbsoluteAxisCode::ABS_Z),
        (
            AbsoluteAxisCode::ABS_THROTTLE,
            |v| linear_map(v, 950, 62000, -32767, 32767, true),
        ),
    );

    axis_map.insert(
        (1, AbsoluteAxisCode::ABS_THROTTLE),
        (
            AbsoluteAxisCode::ABS_BRAKE,
            |v| linear_map(v, 0, 4095, -32767, 32767, true),
        ),
    );

    let axis_map = Arc::new(axis_map);

    // --- Device handler ---
    let spawn_handler = |mut dev: Device, device_id: u8| {
        let vdev = Arc::clone(&vdev);
        let axis_map = Arc::clone(&axis_map);

        thread::spawn(move || loop {
            if let Ok(events) = dev.fetch_events() {
                for ev in events {
                    if ev.event_type() == EventType::ABSOLUTE {
                        let axis = AbsoluteAxisCode(ev.code());

                            if let Some((target, map_fn)) =
                                axis_map.get(&(device_id, axis))
                            {
                                let value = map_fn(ev.value());

                                let mut v = vdev.lock().unwrap();

                                v.emit(&[InputEvent::new(
                                    EventType::ABSOLUTE.0,
                                    target.0,
                                    value,
                                )])
                                    .unwrap();                            }
                    }
                }
            }
        })
    };

    spawn_handler(ursa_minor, 1);
    spawn_handler(twcs, 2);

    loop {
        thread::park();
    }
}