use std::process::ExitCode;
use std::time::{Duration, Instant};

use nusb::transfer::{
    Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Out, Recipient, TransferError,
};
use nusb::{Endpoint, Interface, MaybeFuture};

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

const DESC_BOS: u8 = 0x0f;
const CAPABILITY_PLATFORM: u8 = 0x05;
const MS_OS_20_UUID: [u8; 16] = [
    0xdf, 0x60, 0xdd, 0xd8, 0x89, 0x45, 0xc7, 0x4c, 0x9c, 0xd2, 0x65, 0x9d, 0x9e, 0x64, 0x8a, 0x9f,
];
const MS_OS_20_DESCRIPTOR_INDEX: u16 = 7;

const TIMEOUT: Duration = Duration::from_secs(2);

type Result<T> = std::result::Result<T, String>;

struct Tester {
    device: nusb::Device,
    interface: Interface,
    out: Endpoint<Bulk, Out>,
    input: Endpoint<Bulk, In>,
    packet: usize,
}

fn open() -> Result<Tester> {
    let info = nusb::list_devices()
        .wait()
        .map_err(|e| e.to_string())?
        .find(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
        .ok_or("device not found")?;
    let device = info.open().wait().map_err(|e| format!("open: {e}"))?;
    let interface = device
        .claim_interface(0)
        .wait()
        .map_err(|e| format!("claim: {e}"))?;
    let out = interface
        .endpoint::<Bulk, Out>(EP_OUT)
        .map_err(|e| format!("endpoint out: {e}"))?;
    let input = interface
        .endpoint::<Bulk, In>(EP_IN)
        .map_err(|e| format!("endpoint in: {e}"))?;
    let packet = out.max_packet_size();
    Ok(Tester {
        device,
        interface,
        out,
        input,
        packet,
    })
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
    fn vendor_out(&self, request: u8, value: u16, index: u16, data: &[u8]) -> Result<()> {
        self.interface
            .control_out(
                ControlOut {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request,
                    value,
                    index,
                    data,
                },
                TIMEOUT,
            )
            .wait()
            .map_err(|e| format!("control out 0x{request:02x}: {e}"))
    }

    fn vendor_in(&self, request: u8, length: u16) -> std::result::Result<Vec<u8>, TransferError> {
        self.interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Vendor,
                    recipient: Recipient::Device,
                    request,
                    value: 0,
                    index: 0,
                    length,
                },
                TIMEOUT,
            )
            .wait()
    }

    fn endpoint_halted(&self, endpoint: u8) -> Result<bool> {
        let status = self
            .interface
            .control_in(
                ControlIn {
                    control_type: ControlType::Standard,
                    recipient: Recipient::Endpoint,
                    request: REQUEST_GET_STATUS,
                    value: 0,
                    index: endpoint as u16,
                    length: 2,
                },
                TIMEOUT,
            )
            .wait()
            .map_err(|e| format!("get status: {e}"))?;
        Ok(status.first().copied().unwrap_or(0) & 1 != 0)
    }

    fn stats(&self) -> Result<(u64, u32, u32)> {
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

    fn write(&mut self, data: &[u8], terminate: bool) -> Result<()> {
        self.out.submit(Buffer::from(data.to_vec()));
        if terminate && data.len() % self.packet == 0 {
            self.out.submit(Buffer::new(0));
        }
        while self.out.pending() > 0 {
            let completion = self
                .out
                .wait_next_complete(TIMEOUT)
                .ok_or("write timed out")?;
            completion.status.map_err(|e| format!("write: {e}"))?;
        }
        Ok(())
    }

    fn read(&mut self, length: usize) -> Result<Vec<u8>> {
        self.input.submit(Buffer::new(length));
        let completion = self
            .input
            .wait_next_complete(TIMEOUT)
            .ok_or("read timed out")?;
        completion.status.map_err(|e| format!("read: {e}"))?;
        Ok(completion.buffer.into_vec())
    }

    fn loopback_once(&mut self, rng: &mut Rng, size: usize, transfer: usize) -> Result<()> {
        let mut data = vec![0u8; size];
        rng.fill(&mut data);
        self.write(&data, size < transfer)?;
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

fn info() -> Result<()> {
    let tester = open()?;
    let desc = tester.device.device_descriptor();
    println!(
        "{:04x}:{:04x} bcdDevice {:04x}, speed {:?}",
        desc.vendor_id(),
        desc.product_id(),
        desc.device_version(),
        tester.device.speed()
    );
    for config in tester.device.configurations() {
        for alt in config.interface_alt_settings() {
            println!(
                "interface {} alt {} class {:02x}",
                alt.interface_number(),
                alt.alternate_setting(),
                alt.class()
            );
            for ep in alt.endpoints() {
                println!(
                    "  endpoint {:02x} {:?} {} bytes",
                    ep.address(),
                    ep.transfer_type(),
                    ep.max_packet_size()
                );
            }
        }
    }
    Ok(())
}

fn control() -> Result<()> {
    let tester = open()?;
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
                return Err(format!("echo {size}: wLength 4 returned {} bytes", short.len()));
            }
        }
    }
    match tester.vendor_in(0x7f, 8) {
        Err(TransferError::Stall) => {}
        other => return Err(format!("unknown request: expected stall, got {other:?}")),
    }
    tester.stats()?;
    println!("control: ok");
    Ok(())
}

fn loopback(iterations: usize, kb: u16) -> Result<()> {
    let mut tester = open()?;
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

fn halt() -> Result<()> {
    let mut tester = open()?;
    tester.vendor_out(REQUEST_SET_MODE, MODE_LOOPBACK, 64, &[])?;
    tester
        .interface
        .control_out(
            ControlOut {
                control_type: ControlType::Standard,
                recipient: Recipient::Endpoint,
                request: REQUEST_SET_FEATURE,
                value: FEATURE_ENDPOINT_HALT,
                index: EP_OUT as u16,
                data: &[],
            },
            TIMEOUT,
        )
        .wait()
        .map_err(|e| format!("set halt: {e}"))?;
    if !tester.endpoint_halted(EP_OUT)? {
        return Err("endpoint not halted after SET_FEATURE".into());
    }
    match tester.write(&[0u8; 16], false) {
        Err(e) if e.contains("stalled") => {}
        other => return Err(format!("write to halted endpoint: expected stall, got {other:?}")),
    }
    tester
        .out
        .clear_halt()
        .wait()
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

fn throughput(seconds: u64, mode: u16, kb: u16, chunk: usize, depth: usize) -> Result<()> {
    let mut tester = open()?;
    tester.vendor_out(REQUEST_SET_MODE, mode, kb, &[])?;
    let data = vec![0x5au8; chunk];
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let start = Instant::now();
    let mut sent = 0usize;
    while Instant::now() < deadline || tester.out.pending() > 0 {
        while Instant::now() < deadline && tester.out.pending() < depth {
            tester.out.submit(Buffer::from(data.clone()));
        }
        let completion = tester
            .out
            .wait_next_complete(TIMEOUT)
            .ok_or("write timed out")?;
        completion.status.map_err(|e| format!("write: {e}"))?;
        sent += completion.actual_len;
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

fn msos() -> Result<()> {
    let tester = open()?;
    let bcd_usb = tester.device.device_descriptor().usb_version();
    println!("bcdUSB {bcd_usb:04x}");
    let bos = tester
        .device
        .get_descriptor(DESC_BOS, 0, 0, TIMEOUT)
        .wait()
        .map_err(|e| format!("BOS: {e}"))?;
    if bos.len() < 5 || le16(&bos, 2) != bos.len() {
        return Err(format!("BOS: {} bytes, wTotalLength {}", bos.len(), le16(&bos, 2)));
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
        .interface
        .control_in(
            ControlIn {
                control_type: ControlType::Vendor,
                recipient: Recipient::Device,
                request: vendor_code,
                value: 0,
                index: MS_OS_20_DESCRIPTOR_INDEX,
                length: set_length as u16,
            },
            TIMEOUT,
        )
        .wait()
        .map_err(|e| format!("descriptor set: {e}"))?;
    if set.len() != set_length || set.len() < 10 || le16(&set, 8) != set.len() {
        return Err(format!("descriptor set: {} bytes, BOS says {set_length}", set.len()));
    }
    println!("descriptor set: {} bytes, vendor code 0x{vendor_code:02x}", set.len());
    ms_os_20_features(&set[10..], 1)?;
    Ok(())
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: usb-test <command>
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);
    let number = |i: usize, default: u64| arg(i).and_then(|s| s.parse().ok()).unwrap_or(default);

    let result = match arg(0) {
        Some("info") => info(),
        Some("control") => control(),
        Some("loopback") => loopback(number(1, 200) as usize, number(2, 128) as u16),
        Some("halt") => halt(),
        Some("msos") => msos(),
        Some("throughput") => {
            let mode = match arg(2) {
                Some("internal") => MODE_SINK_INTERNAL,
                _ => MODE_SINK_PSRAM,
            };
            throughput(
                number(1, 5),
                mode,
                number(3, 512) as u16,
                number(4, 1024) as usize * 1024,
                number(5, 8) as usize,
            )
        }
        _ => return usage(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
