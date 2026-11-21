use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_os = "linux")]
        {
        use evdev::FFEffectData;
    }
}

use hidapi::HidDevice;

pub struct HidEffectHandler {
    pub device: HidDevice,
}
impl HidEffectHandler {
    pub fn new(device: HidDevice) -> Self {
        Self { device }
    }

    #[cfg(target_os = "linux")]
    pub fn send_effect(
        &self,
        effect: &FFEffectData,
        value: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match effect.kind {
            evdev::FFEffectKind::Rumble {
                strong_magnitude,
                weak_magnitude,
            } => {
                let bytes: [u8; 14];
                if value {
                    let strong_amount = strong_magnitude as f64 / u16::MAX as f64;
                    let weak_amount = weak_magnitude as f64 / u16::MAX as f64;
                    let total = ((3f64 * strong_amount + weak_amount) / 3f64).clamp(0.0, 1.0);
                    let val = (total * u8::MAX as f64) as u8;
                    // println!("weak: {}, strong: {}, out: {}", weak_magnitude, strong_magnitude, val);
                    bytes = gen_ursa_minor_vib_bytes(val);
                } else {
                    bytes = gen_ursa_minor_vib_bytes(0x00);
                }
                // for i in 0..14{
                //     print!("{:02X} ", bytes[i]);
                // }
                // println!();
                self.device.write(&bytes)?;
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn gen_ursa_minor_vib_bytes(vib: u8) -> [u8; 14] {
    [
        0x02, 0x0a, 0xbf, 0x00, 0x00, 0x03, 0x49, 0x00, vib, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]
}

#[allow(dead_code)]
pub fn gen_ursa_minor_light_bytes(light: u8) -> [u8; 14] {
    [
        0x2, 0x20, 0xbb, 0x0, 0x0, 0x3, 0x49, 0x0, light, 0x0, 0x0, 0x0, 0x0, 0x0,
    ]
}
