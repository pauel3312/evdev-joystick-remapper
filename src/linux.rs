use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_os = "linux")] {
        use std::io::ErrorKind;
        use std::thread;
        use std::time::Duration;
        use crate::{Args, common::HidEffectHandler};

        use evdev::{AbsoluteAxisCode, AbsInfo, UinputAbsSetup, FFEffectCode, AttributeSet, EventSummary, InputId, BusType, KeyCode, FFEffectData};
        use evdev::uinput::VirtualDevice;
        use crate::id_pool::IdPool;

        use std::os::unix::io::AsFd;
        use nix::fcntl::{fcntl, FcntlArg, OFlag};

    }
}

#[cfg(target_os = "linux")]
fn set_nonblocking(dev: &VirtualDevice) -> std::io::Result<()> {
    let fd = dev.as_fd();

    let flags = OFlag::from_bits_truncate(fcntl(&fd, FcntlArg::F_GETFL)?);

    fcntl(&fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))
        .map_err(|e| std::io::Error::new(ErrorKind::Other, e))?;

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn main(args: Args, handler: HidEffectHandler) -> Result<(), Box<dyn std::error::Error>> {
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
    let mut vdev = VirtualDevice::builder()?
        .name("Combined Virtual Device")
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_THROTTLE,
            AbsInfo::new(0, -32767, 32767, 0, 0, 0),
        ))?
        .with_absolute_axis(&UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_BRAKE,
            AbsInfo::new(0, -32767, 32767, 0, 0, 0),
        ))?
        .with_ff_effects_max(16)
        .with_ff(&ff)?
        .with_keys(&btns)?
        .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0110))
        .build()?;

    set_nonblocking(&vdev)?;

    println!(
        "Virtual device created at {}",
        vdev.get_syspath()?.to_str().unwrap()
    );
    println!("Starting vibration forwarder… Press Ctrl+C to exit.");

    let mut effects_map = IdPool::new();
    let mut curr_effect: Option<FFEffectData> = None;

    loop {
        thread::sleep(Duration::from_millis(10));
        let events = {
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
                EventSummary::UInput(uinput_ev, code, _) => {
                    match code {
                        evdev::UInputCode::UI_FF_UPLOAD => {
                            // println!("FF upload");
                            match vdev.process_ff_upload(uinput_ev) {
                                Ok(mut upload) => {
                                    // println!("Effect: {:?}", upload.effect());

                                    let id: i16;
                                    if args.rewired {
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
                            // println!("FF erase");
                            match vdev.process_ff_erase(uinput_ev) {
                                Ok(mut erase) => {
                                    // println!("Erase id={}", erase.effect_id());
                                    if args.rewired {
                                        continue;
                                    }
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

                EventSummary::ForceFeedback(ev, code, value) => {
                    if args.verbose {
                        println!("FF play/stop: {:?}: {:x}, {}", ev, code.0, value);
                    }
                    match code {
                        FFEffectCode::FF_RUMBLE => {}
                        FFEffectCode::FF_PERIODIC => {}
                        FFEffectCode::FF_SPRING => {}
                        FFEffectCode::FF_DAMPER => {}
                        FFEffectCode::FF_SINE => {}
                        FFEffectCode::FF_CONSTANT => {}
                        FFEffectCode::FF_CUSTOM => {}
                        FFEffectCode::FF_GAIN => {}
                        _ => {
                            if args.rewired {
                                if curr_effect == None {
                                    continue;
                                };
                                handler.send_effect(&(curr_effect.unwrap()), value != 0)?;
                            } else {
                                let effect = effects_map.get(code.0 as i16).unwrap();
                                handler.send_effect(effect, value != 0)?;
                            }
                        }
                    }
                }

                _ => {}
            }
        }
    }
}
