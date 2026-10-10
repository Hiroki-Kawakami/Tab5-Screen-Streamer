mod transport;
mod usbip;

use std::process::ExitCode;
use std::time::Instant;

use transport::{Error, Kind, Nusb, Request, Target, Transport};

const VENDOR_ID: u16 = 0xf055;
const PRODUCT_ID: u16 = 0x0000;
const EP_OUT: u8 = 0x01;
const EP_IN: u8 = 0x81;

const REQUEST_SET_MODE: u8 = 0x01;
const REQUEST_STATS: u8 = 0x02;
const REQUEST_ECHO: u8 = 0x03;
const MODE_LOOPBACK: u16 = 0;
const MODE_SINK_PSRAM: u16 = 1;
const MODE_SINK_INTERNAL: u16 = 2;
const ECHO_BYTES: usize = 256;

const FEATURE_ENDPOINT_HALT: u16 = 0;
const REQUEST_GET_STATUS: u8 = 0x00;
const REQUEST_SET_FEATURE: u8 = 0x03;
const REQUEST_GET_DESCRIPTOR: u8 = 0x06;

const DESC_DEVICE: u8 = 0x01;
const DESC_CONFIG: u8 = 0x02;
const DESC_INTERFACE: u8 = 0x04;
const DESC_ENDPOINT: u8 = 0x05;
const DESC_BOS: u8 = 0x0f;
const CAPABILITY_PLATFORM: u8 = 0x05;
const MS_OS_20_UUID: [u8; 16] = [
    0xdf, 0x60, 0xdd, 0xd8, 0x89, 0x45, 0xc7, 0x4c, 0x9c, 0xd2, 0x65, 0x9d, 0x9e, 0x64, 0x8a, 0x9f,
];
const MS_OS_20_DESCRIPTOR_INDEX: u16 = 7;

type Result<T> = std::result::Result<T, String>;

struct Tester {
    t: Box<dyn Transport>,
    packet: usize,
}

fn open(usbip: Option<&str>) -> Result<Tester> {
    let t: Box<dyn Transport> = match usbip {
        Some(address) => Box::new(usbip::Usbip::connect(address, EP_OUT, EP_IN)?),
        None => Box::new(Nusb::open(VENDOR_ID, PRODUCT_ID, EP_OUT, EP_IN)?),
    };
    let packet = t.max_packet();
    if packet == 0 {
        return Err("no bulk OUT endpoint".into());
    }
    Ok(Tester { t, packet })
}

fn vendor(request: u8, value: u16, index: u16) -> Request {
    Request {
        kind: Kind::Vendor,
        target: Target::Device,
        request,
        value,
        index,
    }
}

fn get_descriptor(kind: u8, index: u8) -> Request {
    Request {
        kind: Kind::Standard,
        target: Target::Device,
        request: REQUEST_GET_DESCRIPTOR,
        value: (kind as u16) << 8 | index as u16,
        index: 0,
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn fill(&mut self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_mut(8) {
            let value = self.next().to_le_bytes();
            chunk.copy_from_slice(&value[..chunk.len()]);
        }
    }
}

impl Tester {
    fn vendor_out(&mut self, request: u8, value: u16, index: u16, data: &[u8]) -> Result<()> {
        self.t
            .control_out(vendor(request, value, index), data)
            .map_err(|e| format!("control out 0x{request:02x}: {e}"))
    }

    fn vendor_in(&mut self, request: u8, length: u16) -> std::result::Result<Vec<u8>, Error> {
        self.t.control_in(vendor(request, 0, 0), length)
    }

    fn descriptor(&mut self, kind: u8, length: u16) -> Result<Vec<u8>> {
        self.t
            .control_in(get_descriptor(kind, 0), length)
            .map_err(|e| format!("descriptor 0x{kind:02x}: {e}"))
    }

    fn endpoint_halted(&mut self, endpoint: u8) -> Result<bool> {
        let request = Request {
            kind: Kind::Standard,
            target: Target::Endpoint,
            request: REQUEST_GET_STATUS,
            value: 0,
            index: endpoint as u16,
        };
        let status = self
            .t
            .control_in(request, 2)
            .map_err(|e| format!("get status: {e}"))?;
        Ok(status.first().copied().unwrap_or(0) & 1 != 0)
    }

    fn stats(&mut self) -> Result<(u64, u32, u32)> {
        let data = self
            .vendor_in(REQUEST_STATS, 16)
            .map_err(|e| format!("stats: {e}"))?;
        if data.len() != 16 {
            return Err(format!("stats: {} bytes", data.len()));
        }
        Ok((
            u64::from_le_bytes(data[0..8].try_into().unwrap()),
            u32::from_le_bytes(data[8..12].try_into().unwrap()),
            u32::from_le_bytes(data[12..16].try_into().unwrap()),
        ))
    }

    fn write(&mut self, data: &[u8], terminate: bool) -> std::result::Result<(), Error> {
        self.t.submit_out(data.to_vec());
        if terminate && data.len() % self.packet == 0 {
            self.t.submit_out(Vec::new());
        }
        let mut result = Ok(());
        while self.t.pending_out() > 0 {
            if let Err(e) = self.t.wait_out() {
                result = result.and(Err(e));
            }
        }
        result
    }

    fn read(&mut self, length: usize) -> Result<Vec<u8>> {
        self.t.submit_in(length);
        self.t.wait_in().map_err(|e| format!("read: {e}"))
    }

    fn loopback_once(&mut self, rng: &mut Rng, size: usize, transfer: usize) -> Result<()> {
        let mut data = vec![0u8; size];
        rng.fill(&mut data);
        self.write(&data, size < transfer)
            .map_err(|e| format!("write: {e}"))?;
        let echoed = self.read(transfer)?;
        if echoed != data {
            let at = echoed
                .iter()
                .zip(&data)
                .position(|(a, b)| a != b)
                .unwrap_or(echoed.len().min(data.len()));
            return Err(format!(
                "{size} bytes: echoed {} bytes, first difference at {at}",
                echoed.len()
            ));
        }
        Ok(())
    }
}

fn info(tester: &mut Tester) -> Result<()> {
    let device = tester.descriptor(DESC_DEVICE, 18)?;
    if device.len() < 18 {
        return Err("short device descriptor".into());
    }
    println!(
        "{:04x}:{:04x} bcdDevice {:04x}, speed {}",
        le16(&device, 8),
        le16(&device, 10),
        le16(&device, 12),
        tester.t.speed()
    );
    let header = tester.descriptor(DESC_CONFIG, 9)?;
    let config = tester.descriptor(DESC_CONFIG, le16(&header, 2) as u16)?;
    let mut pos = 0;
    while pos + 1 < config.len() && config[pos] > 0 {
        match config[pos + 1] {
            DESC_INTERFACE => println!(
                "interface {} alt {} class {:02x}",
                config[pos + 2],
                config[pos + 3],
                config[pos + 5]
            ),
            DESC_ENDPOINT => {
                let kind =
                    ["Control", "Isochronous", "Bulk", "Interrupt"][config[pos + 3] as usize & 3];
                println!(
                    "  endpoint {:02x} {kind} {} bytes",
                    config[pos + 2],
                    le16(&config, pos + 4) & 0x7ff
                );
            }
            _ => {}
        }
        pos += config[pos] as usize;
    }
    Ok(())
}

fn control(tester: &mut Tester) -> Result<()> {
    let mut rng = Rng(0x1234_5678_9abc_def0);
    for size in [0, 1, 8, 63, 64, 65, 127, 128, 200, ECHO_BYTES] {
        let mut data = vec![0u8; size];
        rng.fill(&mut data);
        tester.vendor_out(REQUEST_ECHO, 0, 0, &data)?;
        let echoed = tester
            .vendor_in(REQUEST_ECHO, ECHO_BYTES as u16)
            .map_err(|e| format!("echo in: {e}"))?;
        if echoed != data {
            return Err(format!("echo {size}: got {} bytes back", echoed.len()));
        }
        if size > 4 {
            let short = tester
                .vendor_in(REQUEST_ECHO, 4)
                .map_err(|e| format!("echo in (4): {e}"))?;
            if short != data[..4] {
                return Err(format!(
                    "echo {size}: wLength 4 returned {} bytes",
                    short.len()
                ));
            }
        }
    }
    match tester.vendor_in(0x7f, 8) {
        Err(Error::Stall) => {}
        other => return Err(format!("unknown request: expected stall, got {other:?}")),
    }
    tester.stats()?;
    println!("control: ok");
    Ok(())
}

fn loopback(tester: &mut Tester, iterations: usize, kb: u16) -> Result<()> {
    tester.vendor_out(REQUEST_SET_MODE, MODE_LOOPBACK, kb, &[])?;
    let transfer = kb as usize * 1024;
    let packet = tester.packet;
    let fixed = [
        1,
        packet - 1,
        packet,
        packet + 1,
        2 * packet,
        65023,
        65024,
        65025,
        65536,
        70000,
        130048,
        transfer - 1,
        transfer,
    ];
    let mut rng = Rng(0x0f0e_0d0c_0b0a_0908);
    let mut total = 0usize;
    for i in 0..iterations {
        let size = if i < fixed.len() {
            fixed[i]
        } else {
            1 + rng.next() as usize % transfer
        };
        if size > transfer {
            continue;
        }
        tester
            .loopback_once(&mut rng, size, transfer)
            .map_err(|e| format!("iteration {i}: {e}"))?;
        total += size;
    }
    let (bytes, transfers, errors) = tester.stats()?;
    println!(
        "loopback: {iterations} iterations, {total} bytes ok; device saw {bytes} bytes in {transfers} transfers, {errors} errors"
    );
    Ok(())
}

fn halt(tester: &mut Tester) -> Result<()> {
    tester.vendor_out(REQUEST_SET_MODE, MODE_LOOPBACK, 64, &[])?;
    let set_halt = Request {
        kind: Kind::Standard,
        target: Target::Endpoint,
        request: REQUEST_SET_FEATURE,
        value: FEATURE_ENDPOINT_HALT,
        index: EP_OUT as u16,
    };
    tester
        .t
        .control_out(set_halt, &[])
        .map_err(|e| format!("set halt: {e}"))?;
    if !tester.endpoint_halted(EP_OUT)? {
        return Err("endpoint not halted after SET_FEATURE".into());
    }
    match tester.write(&[0u8; 16], false) {
        Err(Error::Stall) => {}
        other => {
            return Err(format!(
                "write to halted endpoint: expected stall, got {other:?}"
            ))
        }
    }
    tester
        .t
        .clear_halt_out()
        .map_err(|e| format!("clear halt: {e}"))?;
    if tester.endpoint_halted(EP_OUT)? {
        return Err("endpoint still halted after CLEAR_FEATURE".into());
    }
    let mut rng = Rng(42);
    tester.loopback_once(&mut rng, 1000, 64 * 1024)?;
    tester.loopback_once(&mut rng, 4096, 64 * 1024)?;
    println!("halt: ok");
    Ok(())
}

fn throughput(
    tester: &mut Tester,
    seconds: u64,
    mode: u16,
    kb: u16,
    chunk: usize,
    depth: usize,
) -> Result<()> {
    tester.vendor_out(REQUEST_SET_MODE, mode, kb, &[])?;
    let data = vec![0x5au8; chunk];
    let deadline = Instant::now() + std::time::Duration::from_secs(seconds);
    let start = Instant::now();
    let mut sent = 0usize;
    while Instant::now() < deadline || tester.t.pending_out() > 0 {
        while Instant::now() < deadline && tester.t.pending_out() < depth {
            tester.t.submit_out(data.clone());
        }
        sent += tester.t.wait_out().map_err(|e| format!("write: {e}"))?;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let (bytes, transfers, errors) = tester.stats()?;
    println!(
        "throughput: {:.1} MB/s ({sent} bytes in {elapsed:.2} s); device counted {bytes} bytes in {transfers} transfers, {errors} errors",
        sent as f64 / elapsed / 1e6
    );
    tester.vendor_out(REQUEST_SET_MODE, MODE_LOOPBACK, 0, &[])?;
    Ok(())
}

fn le16(bytes: &[u8], at: usize) -> usize {
    u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize
}

fn utf16z(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn ms_os_20_features(set: &[u8], depth: usize) -> Result<()> {
    let indent = "  ".repeat(depth);
    let mut pos = 0;
    while pos < set.len() {
        if pos + 4 > set.len() {
            return Err(format!("truncated descriptor at {pos}"));
        }
        let length = le16(set, pos);
        let kind = le16(set, pos + 2);
        let end = match kind {
            0x01 | 0x02 => pos + le16(set, pos + 6),
            _ => pos + length,
        };
        if length < 4 || end > set.len() {
            return Err(format!("bad length at {pos}"));
        }
        match kind {
            0x01 => {
                println!("{indent}configuration subset {}", set[pos + 4]);
                ms_os_20_features(&set[pos + length..end], depth + 1)?;
            }
            0x02 => {
                println!("{indent}function subset, first interface {}", set[pos + 4]);
                ms_os_20_features(&set[pos + length..end], depth + 1)?;
            }
            0x03 => {
                let id = String::from_utf8_lossy(&set[pos + 4..pos + 12]);
                println!("{indent}compatible id {}", id.trim_end_matches('\0'));
            }
            0x04 => {
                let name_len = le16(set, pos + 6);
                let name = utf16z(&set[pos + 8..pos + 8 + name_len]);
                let data_at = pos + 8 + name_len;
                let data_len = le16(set, data_at);
                let data = utf16z(&set[data_at + 2..data_at + 2 + data_len]);
                println!("{indent}registry {name} = {data}");
            }
            other => println!("{indent}feature 0x{other:02x}, {length} bytes"),
        }
        pos = end;
    }
    Ok(())
}

fn msos(tester: &mut Tester) -> Result<()> {
    let device = tester.descriptor(DESC_DEVICE, 18)?;
    println!("bcdUSB {:04x}", le16(&device, 2));
    let bos = tester.descriptor(DESC_BOS, 4096)?;
    if bos.len() < 5 || le16(&bos, 2) != bos.len() {
        return Err(format!(
            "BOS: {} bytes, wTotalLength {}",
            bos.len(),
            le16(&bos, 2)
        ));
    }
    let mut platform = None;
    let mut pos = 5;
    for _ in 0..bos[4] {
        let length = bos[pos] as usize;
        let kind = bos[pos + 2];
        println!("capability 0x{kind:02x}, {length} bytes");
        if kind == CAPABILITY_PLATFORM && bos[pos + 4..pos + 20] == MS_OS_20_UUID {
            platform = Some((le16(&bos, pos + 24), bos[pos + 26]));
        }
        pos += length;
    }
    let (set_length, vendor_code) = platform.ok_or("no MS OS 2.0 platform capability")?;
    let set = tester
        .t
        .control_in(
            vendor(vendor_code, 0, MS_OS_20_DESCRIPTOR_INDEX),
            set_length as u16,
        )
        .map_err(|e| format!("descriptor set: {e}"))?;
    if set.len() != set_length || set.len() < 10 || le16(&set, 8) != set.len() {
        return Err(format!(
            "descriptor set: {} bytes, BOS says {set_length}",
            set.len()
        ));
    }
    println!(
        "descriptor set: {} bytes, vendor code 0x{vendor_code:02x}",
        set.len()
    );
    ms_os_20_features(&set[10..], 1)?;
    Ok(())
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: usb-test [--usbip <host:port>] <command>
  info
  control
  loopback [iterations] [transfer-kb]
  halt
  msos
  throughput [seconds] [psram|internal] [transfer-kb] [host-chunk-kb] [depth]"
    );
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut usbip = None;
    if args.first().map(String::as_str) == Some("--usbip") {
        if args.len() < 2 {
            return usage();
        }
        usbip = Some(args[1].clone());
        args.drain(..2);
    }
    let arg = |i: usize| args.get(i).map(String::as_str);
    let number = |i: usize, default: u64| arg(i).and_then(|s| s.parse().ok()).unwrap_or(default);

    let command = match arg(0) {
        Some(c @ ("info" | "control" | "loopback" | "halt" | "msos" | "throughput")) => c,
        _ => return usage(),
    };
    let result = open(usbip.as_deref()).and_then(|mut tester| match command {
        "info" => info(&mut tester),
        "control" => control(&mut tester),
        "loopback" => loopback(&mut tester, number(1, 200) as usize, number(2, 128) as u16),
        "halt" => halt(&mut tester),
        "msos" => msos(&mut tester),
        _ => {
            let mode = match arg(2) {
                Some("internal") => MODE_SINK_INTERNAL,
                _ => MODE_SINK_PSRAM,
            };
            throughput(
                &mut tester,
                number(1, 5),
                mode,
                number(3, 512) as u16,
                number(4, 1024) as usize * 1024,
                number(5, 8) as usize,
            )
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
