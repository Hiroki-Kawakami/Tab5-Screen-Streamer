use std::io::Write;
use clap::Parser;

mod capture;
mod input;

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

    let (mut device, reader) = usb_device::open(args.wait_device)
        .expect("Device Open Failed!");

    // Tab5 -> PC touch input: read reports off the vendor IN endpoint and inject
    // OS pointer events. The target display geometry is published by the capture
    // backend once it resolves the display, so early reports are simply dropped.
    std::thread::spawn(move || input::run(reader));

    let config = capture::CaptureConfig {
        display: args.display,
        quality: args.quality,
    };
    capture::start(config, move |capture_context| {
        let mut frames: usize = 0;
        let mut transferred: usize = 0;
        loop {
            let frame = capture_context.get_frame();
            let result: Result<(), std::io::Error> = (|| {
                device.write_all(&frame.data[..frame.data_size])?;
                device.flush()?;
                Ok(())
            })();
            if result.is_ok() {
                transferred += frame.data_size;
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
    use nusb::{MaybeFuture, hotplug::HotplugEvent, io::{EndpointRead, EndpointWrite}, transfer::{Bulk, In, Out}};

    pub const VID: u16 = 0xf055;
    pub const PID: u16 = 0x1118;
    pub const EP_OUT: u8 = 0x01;
    pub const EP_IN: u8 = 0x81;
    /// Expected device version (bcdDevice in the USB device descriptor).
    /// Must match the firmware's `.bcdDevice` in `usb_descriptors.c`.
    pub const DEVICE_VERSION: u16 = 0x0300;

    fn wait_device_arrived() -> Result<(), nusb::Error> {
        println!("Waiting device connection...");
        let watcher = nusb::watch_devices()?;
        for event in futures_lite::stream::block_on(watcher) {
            match event {
                HotplugEvent::Connected(d) => {
                    if d.product_id() == PID && d.vendor_id() == VID { break }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn find_device() -> Option<nusb::DeviceInfo> {
        nusb::list_devices()
            .wait()
            .unwrap()
            .find(|d| d.vendor_id() == VID && d.product_id() == PID)
    }

    pub fn open(wait: bool) -> Result<(EndpointWrite<Bulk>, EndpointRead<Bulk>), nusb::Error> {
        let mut device_info = find_device();
        if wait && device_info.is_none() {
            wait_device_arrived()?;
            device_info = find_device();
        }
        let device_info = device_info
            .expect("Device not found!");

        let version = device_info.device_version();
        if version != DEVICE_VERSION {
            eprintln!(
                "Device version mismatch: expected 0x{:04X}, found 0x{:04X}.\n\
                 The PC tool and the Tab5 firmware are out of sync; \
                 please update them to matching versions.",
                DEVICE_VERSION, version
            );
            std::process::exit(1);
        }

        let device = device_info.open().wait().unwrap();
        let interface = device.claim_interface(0).wait().unwrap();
        // OUT: video stream (PC -> Tab5). IN: touch reports (Tab5 -> PC).
        let writer = interface
            .endpoint::<Bulk, Out>(EP_OUT).unwrap()
            .writer(8192).with_num_transfers(8);
        let reader = interface
            .endpoint::<Bulk, In>(EP_IN).unwrap()
            .reader(4096).with_num_transfers(4);
        Ok((writer, reader))
    }
}
