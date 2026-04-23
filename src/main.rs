mod id_pool;

use id_pool::IdPool;

use evdev::{Device, InputEvent, EventType, AbsoluteAxisCode, AbsInfo, UinputAbsSetup, FFEffectCode, AttributeSet, EventSummary, InputId, BusType, KeyCode, FFEffectData};
use evdev::uinput::VirtualDevice;


use std::os::unix::io::AsRawFd;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'r', long = "rewired")]
    rewired: bool,
}


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

    let args = Args::parse();

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

    let mut ff = AttributeSet::<FFEffectCode>::new();
    ff.insert(FFEffectCode::FF_RUMBLE);
    ff.insert(FFEffectCode::FF_CUSTOM);
    ff.insert(FFEffectCode::FF_GAIN);
    ff.insert(FFEffectCode::FF_PERIODIC);
    ff.insert(FFEffectCode::FF_SPRING);
    ff.insert(FFEffectCode::FF_DAMPER);
    ff.insert(FFEffectCode::FF_SINE);
    ff.insert(FFEffectCode::FF_CONSTANT);

    let mut btns = AttributeSet::new();
    btns.insert(KeyCode::BTN_SOUTH);
    btns.insert(KeyCode::BTN_EAST);
    btns.insert(KeyCode::BTN_NORTH);
    btns.insert(KeyCode::BTN_WEST);
    btns.insert(KeyCode::BTN_TR);
    btns.insert(KeyCode::BTN_TL);
    btns.insert(KeyCode::BTN_SELECT);
    btns.insert(KeyCode::BTN_START);
    btns.insert(KeyCode::BTN_THUMBL);
    btns.insert(KeyCode::BTN_THUMBR);


    // --- Create virtual device ---
    let vdev = VirtualDevice::builder()?
        .name("Combined Virtual Device")
        .with_absolute_axis(
            &UinputAbsSetup::new(
                AbsoluteAxisCode::ABS_THROTTLE,
                AbsInfo::new(0, -32767, 32767, 0, 0, 0),
            )
        )?
        .with_absolute_axis(
            &UinputAbsSetup::new(
                AbsoluteAxisCode::ABS_BRAKE,
                AbsInfo::new(0, -32767, 32767, 0, 0, 0),
            )
        )?
        .with_ff_effects_max(16)
        .with_ff(&ff)?
        .with_keys(&btns)?
        .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110))
        .build()?;

    set_nonblocking(&vdev)?;


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


    let spawn_out_handler = |mut dev: Device, device_id: u8| {
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
                            println!("{}", ev.value());
                            let value = map_fn(ev.value());

                            let mut v = vdev.lock().unwrap();

                            v.emit(&[InputEvent::new(
                                EventType::ABSOLUTE.0,
                                target.0,
                                value,
                            )])
                                .unwrap();
                        }
                    }
                }
            }
        })
    };

    spawn_out_handler(ursa_minor, 1);
    spawn_out_handler(twcs, 2);
    spawn_ff_thread(vdev, args.rewired);

    loop {
        thread::park();
    }
}

fn spawn_ff_thread(
    vdev_orig: Arc<Mutex<VirtualDevice>>,
    rewired: bool
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut effects_map = IdPool::new();
        let mut curr_effect: Option<FFEffectData> = None;

        loop {
            thread::sleep(Duration::from_millis(10));
            let events = {
                let mut vdev = vdev_orig.lock().unwrap();
                match vdev.fetch_events() {
                    Ok(iter) => iter.collect::<Vec<_>>(),

                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        // No events available right now → this is NORMAL
                        continue;
                    }

                    Err(e) => {
                        eprintln!("fetch_events error: {}", e);
                        continue;
                    }
                }
            };
            for event in events {
                match event.destructure() {
                    EventSummary::ForceFeedback(ev, code, value) => {
                        println!("FF play/stop: {:?}: {:x}, {}",ev, code.0, value);
                        if rewired{
                            if curr_effect == None { continue };
                            handle_effect(curr_effect.unwrap())
                        } else {
                            let effect = effects_map.get(code.0 as i16).unwrap();
                            handle_effect(*effect);
                        }
                    }

                    EventSummary::UInput(uinput_ev, code, _) => {

                        match code {
                            evdev::UInputCode::UI_FF_UPLOAD => {
                                println!("FF upload");
                                let mut vdev = vdev_orig.lock().unwrap();
                                match vdev.process_ff_upload(uinput_ev) {
                                    Ok(mut upload) => {
                                        println!("Effect: {:?}", upload.effect());

                                        let id: i16;
                                        if rewired {
                                            id = 0;
                                            curr_effect = Some(upload.effect());
                                        } else {
                                            id = effects_map.insert(upload.effect());
                                        }
                                        upload.set_effect_id(id);
                                        upload.set_retval(0);
                                    }
                                    Err(e) => {
                                        eprintln!("upload error: {}", e);
                                    }
                                }
                            }

                            evdev::UInputCode::UI_FF_ERASE => {
                                println!("FF erase");
                                let mut vdev = vdev_orig.lock().unwrap();
                                match vdev.process_ff_erase(uinput_ev) {
                                    Ok(mut erase) => {
                                        println!("Erase id={}", erase.effect_id());
                                        if rewired {continue}
                                        effects_map.remove(erase.effect_id() as i16);
                                        erase.set_retval(0);
                                    }
                                    Err(e) => {
                                        eprintln!("erase error: {}", e);
                                    }
                                }
                            }

                            _ => {}
                        }
                    }

                    _ => {}
                }
            }
        }
    })
}

fn set_nonblocking(dev: &VirtualDevice) -> std::io::Result<()> {
    let fd = dev.as_raw_fd();

    let flags = OFlag::from_bits_truncate(
        fcntl(fd, FcntlArg::F_GETFL)?
    );

    fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))
        .map_err(|e| std::io::Error::new(ErrorKind::Other, e))?;

    Ok(())
}


fn handle_effect(_: FFEffectData){} // TODO
