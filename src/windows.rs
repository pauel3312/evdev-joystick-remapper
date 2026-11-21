use std::thread;
use std::time::Duration;
use vigem_rust::{Client, X360Report};
use crate::Args;
use crate::common::{HidEffectHandler, gen_ursa_minor_vib_bytes};

pub fn main(args: Args, handler: HidEffectHandler) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::connect()?;
    println!("Connected to ViGEmBus");

    let target = client.new_x360_target().plug()?.wait_for_ready()?;
    println!("Virtual Xbox 360 controller plugged in");

    let receiver = target.register_notification()?;
    println!("Subscribed to controller notifications");
    println!("Waiting for haptic feedback events... Press Ctrl+C to exit.");

    thread::spawn(move || {
        while let Ok(n) = receiver.recv() {
            if args.verbose
            {
                println!(
                    "rumble L={} R={} led={}",
                    n.large_motor, n.small_motor, n.led_number
                );
            }
            let total = ((3u16 * n.large_motor as u16 + n.small_motor as u16) / 4) as u8;
            
            match handler.device.write(&gen_ursa_minor_vib_bytes(total)) {
                Ok(_) => {
                }
                Err(e) => {
                    eprintln!("Failed to write vibration data: {}", e);
                }
            }
        }
    });

    // Sticks are signed i16 (+/-32767). Buttons stay clear: face/D-pad spam will
    // navigate Steam, games, and any focused XInput client.
    let mut report = X360Report::default();
    let mut angle: f64 = 0.0;

    loop {
        if args.spin
        {
            // Slow circle on the left stick so a gamepad tester shows motion.
            angle += 0.05;
            let (sin, cos) = angle.sin_cos();
            report.thumb_lx = (sin * 32767.0) as i16;
            report.thumb_ly = (cos * 32767.0) as i16;
        }
        target.update(&report)?;

        thread::sleep(Duration::from_millis(16));
    }
}
