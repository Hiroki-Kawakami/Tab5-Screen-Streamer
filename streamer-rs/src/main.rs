use std::time::Duration;
use clap::Parser;
use rusb::{DeviceHandle, GlobalContext};

mod capture;

const VID: u16 = 0x303a;
const PID: u16 = 0x4020;
const EP_OUT: u8 = 0x01;

#[derive(Parser, Debug)]
#[command(version, about, author = "Hiroki Kawakami")]
struct Args {
    /// Display Select
    #[arg(long)]
    display: Option<usize>,
}

fn main() {
    let args = Args::parse();

    if !capture::check_permission() {
        println!("Platform not supported!");
        return;
    }

    let device = open_device().expect("Device Open Failed!");
    let capture_context = capture::start(args.display);

    let mut frames: usize = 0;
    let mut transferred: usize = 0;
    loop {
        let frame = capture_context.get_frame();
        if let Ok(size) = device.write_bulk(EP_OUT, &frame.data[..frame.data_size], Duration::from_secs(1)) {
            transferred += size;
            frames += 1;
        } else {
            panic!("USB Tx Failed!")
        }
        if let Some(fps) = frame.fps {
            let speed = transferred / 1000;
            println!("Capture: {}fps, USB Tx: {}fps, {}kB/s, quality={}", fps, frames, speed, frame.quality);
            frames = 0;
            transferred = 0;
        }
    }
}

fn open_device() -> Result<DeviceHandle<GlobalContext>, rusb::Error> {
    let device = rusb::open_device_with_vid_pid(VID, PID)
        .expect("Device not found!");
    let _ = device.detach_kernel_driver(0);
    device.set_active_configuration(1)?;
    device.claim_interface(0)?;
    Ok(device)
}
