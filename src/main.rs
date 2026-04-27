mod id_pool;

use id_pool::IdPool;

use evdev::{AbsoluteAxisCode, AbsInfo, UinputAbsSetup, FFEffectCode, AttributeSet, EventSummary, InputId, BusType, KeyCode, FFEffectData};
use evdev::uinput::VirtualDevice;
use hidapi::{HidApi, HidDevice};


use std::os::unix::io::AsRawFd;
use nix::fcntl::{fcntl, FcntlArg, OFlag};
use std::io::ErrorKind;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {

    let args = Args::parse();

    let api = HidApi::new()?;

    let ursa_minor_hidraw = api.open(0x4098, 0xbc2a)?;

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

    println!("Found {}", ursa_minor_hidraw.get_device_info()?.product_string().unwrap());
    println!("Virtual device created at {}", vdev.get_syspath()?.to_str().unwrap());
    println!("Starting input merger… Press Ctrl+C to exit.");

    let mut effects_map = IdPool::new();
    let mut curr_effect: Option<FFEffectData> = None;

    let handler = HidEffectHandler::new(ursa_minor_hidraw);


    loop {
        thread::sleep(Duration::from_millis(10));
        let events = {
            // let mut vdev = vdev_orig.lock().unwrap();
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
                                    if args.rewired {continue}
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


                EventSummary::ForceFeedback(_ev, code, value) => {
                    // println!("FF play/stop: {:?}: {:x}, {}",ev, code.0, value);
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
                            if args.rewired{
                                if curr_effect == None { continue };
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

fn gen_ursa_minor_vib_bytes(vib: u8) -> [u8; 14] {
    [
        0x02, 0x0a, 0xbf, 0x00, 0x00, 0x03, 0x49, 0x00, vib, 0x00, 0x00, 0x00, 0x00, 0x00
    ]
}

fn gen_ursa_minor_light_bytes(light: u8) -> [u8; 14] {
    [
        0x2, 0x20, 0xbb, 0x0, 0x0, 0x3, 0x49, 0x0, light, 0x0, 0x0, 0x0, 0x0, 0x0
    ]
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

struct HidEffectHandler {
    device: HidDevice,
}
impl HidEffectHandler {
    pub fn new(device: HidDevice) -> Self {
        Self { device }
    }

    pub fn send_effect(&self, effect: &FFEffectData, value: bool) -> Result<(), Box<dyn std::error::Error>> {
        match effect.kind {
            evdev::FFEffectKind::Rumble {
                strong_magnitude, weak_magnitude
            } => {
                let bytes: [u8; 14];
                if value {
                    let strong_amount = strong_magnitude as f64/u16::MAX as f64;
                    let weak_amount = weak_magnitude as f64/u16::MAX as f64;
                    let total = ((3f64*strong_amount + weak_amount)/3f64).clamp(0.0, 1.0);
                    let val = (total*u8::MAX as f64) as u8;
                    // println!("weak: {}, strong: {}, out: {}", weak_magnitude, strong_magnitude, val);
                    bytes = gen_ursa_minor_vib_bytes(val);
                }
                else{
                    bytes = gen_ursa_minor_vib_bytes(0x00);
                }
                // for i in 0..14{
                //     print!("{:02X} ", bytes[i]);
                // }
                // println!();
                self.device.write(&bytes)?;
            },
            _ => {}

        }
        Ok(())
    }
}