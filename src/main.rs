mod common;
mod id_pool;
mod linux;
mod windows;

use hidapi::HidApi;

use crate::common::HidEffectHandler;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[cfg(target_os = "linux")]
    #[arg(short = 'r', long = "rewired")]
    rewired: bool,
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[cfg(target_os = "windows")]
    #[arg(short = 's', long = "spin")]
    spin: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    let api = HidApi::new()?;

    let ursa_minor_hidraw = api.open(0x4098, 0xbc2a)?;


    println!(
        "Found {}",
        ursa_minor_hidraw
            .get_device_info()?
            .product_string()
            .unwrap()
    );

    let handler = HidEffectHandler::new(ursa_minor_hidraw);

    #[cfg(target_os = "linux")]
    return crate::linux::main(args, handler);
    #[cfg(target_os = "windows")]
    return windows::main(args, handler);
}
