use std::time::Duration;
use clap::Parser;

mod capture;

#[derive(Parser, Debug)]
#[command(version, about, author = "Hiroki Kawakami")]
struct Args {
    /// Display Select
    #[arg(long)]
    display: Option<usize>,

    /// Wait device connection
    #[arg(long, default_value_t = false)]
    wait_device: bool,

    // Jpeg Compression Quality (default is dynamic)
    #[arg(long)]
    quality: Option<i32>,

    /// Output performance log
    #[arg(short, default_value_t = false)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();

    if !capture::check_permission() {
        println!("Platform not supported!");
        return;
    }
    if let Some(q) = args.quality && (q < 1 || q > 100) {
        panic!("Invalid quality value: {}", q);
    }

    let device = usb_device::open(args.wait_device)
        .expect("Device Open Failed!");

    let config = capture::CaptureConfig {
        display: args.display,
        quality: args.quality,
    };
    capture::start(config, move |capture_context| {
        let mut frames: usize = 0;
        let mut transferred: usize = 0;
        loop {
            let frame = capture_context.get_frame();
            if let Ok(size) = device.write_bulk(usb_device::EP_OUT, &frame.data[..frame.data_size], Duration::from_secs(1)) {
                transferred += size;
                frames += 1;
            } else {
                panic!("USB Tx Failed!")
            }
            if let Some(fps) = frame.fps {
                if args.verbose {
                    let speed = transferred / 1000;
                    println!("Capture: {}fps, USB Tx: {}fps, {}kB/s, quality={}", fps, frames, speed, frame.quality);
                }
                frames = 0;
                transferred = 0;
            }
        }
    });
}

mod usb_device {
    use std::{sync::mpsc, time::Duration};
    use rusb::{DeviceHandle, GlobalContext, UsbContext};

    pub const VID: u16 = 0x303a;
    pub const PID: u16 = 0x4020;
    pub const EP_OUT: u8 = 0x01;

    struct HotplugCallback {
        tx: mpsc::Sender<()>,
    }
    impl rusb::Hotplug<rusb::GlobalContext> for HotplugCallback {
        fn device_arrived(&mut self, device: rusb::Device<rusb::GlobalContext>) {
            println!("Device connected: {}:{}", device.bus_number(), device.address());
            let _ = self.tx.send(());
        }
        fn device_left(&mut self, _device: rusb::Device<rusb::GlobalContext>) {
        }
    }

    fn wait_device_arrived() -> Result<(), rusb::Error> {
        let ctx = rusb::GlobalContext::default();
        let (tx, rx) = mpsc::channel::<()>();
        let callback = HotplugCallback { tx };
        let reg = rusb::HotplugBuilder::new()
            .vendor_id(VID)
            .product_id(PID)
            .enumerate(true)
            .register(ctx, Box::new(callback))?;
        loop {
            ctx.handle_events(None)?;
            if let Ok(_) = rx.recv_timeout(Duration::ZERO) {
                break;
            }
        }
        ctx.unregister_callback(reg);
        Ok(())
    }

    pub fn open(wait: bool) -> Result<DeviceHandle<GlobalContext>, rusb::Error> {
        let mut device = rusb::open_device_with_vid_pid(VID, PID);
        if wait && device.is_none() {
            if !rusb::has_hotplug() {
                panic!("Platform does not support hotplug!");
            }
            wait_device_arrived()?;
            device = rusb::open_device_with_vid_pid(VID, PID);
        }
        let device = device.expect("Device not found!");
        let _ = device.detach_kernel_driver(0);
        device.set_active_configuration(1)?;
        device.claim_interface(0)?;
        Ok(device)
    }
}
