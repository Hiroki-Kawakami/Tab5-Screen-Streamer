//! Messages to the device and its events, for the device worker.

use js_sys::Array;
use streamer_core::protocol::{self, AudioClock, Event, Info, Volume};
use wasm_bindgen::prelude::*;

use crate::object;

#[wasm_bindgen]
pub fn message(kind: u8, seq: u32, epoch: u32, payload: &[u8]) -> Vec<u8> {
    protocol::message(kind, seq, epoch, payload)
}

#[wasm_bindgen(js_name = setBrightness)]
pub fn set_brightness(brightness: u8) -> Vec<u8> {
    protocol::set_brightness(brightness)
}

#[wasm_bindgen(js_name = setVolume)]
pub fn set_volume(stream: u8, mute: bool, volume: i16) -> Vec<u8> {
    protocol::set_volume(stream, mute, volume)
}

/// `host` plays it in the host's clock rather than the device's.
#[wasm_bindgen(js_name = audioFormat)]
pub fn audio_format(rate: u32, channels: u8, bits: u8, host: bool) -> Vec<u8> {
    let clock = match host {
        true => AudioClock::Host,
        false => AudioClock::Device,
    };
    protocol::audio_format(rate, channels, bits, clock)
}

#[wasm_bindgen(js_name = errorName)]
pub fn error_name(code: u8) -> String {
    protocol::error_name(code).into()
}

fn volume(volume: &Volume) -> JsValue {
    object(&[
        ("mute", volume.mute.into()),
        ("volume", volume.volume.into()),
        ("min", volume.min.into()),
        ("max", volume.max.into()),
    ])
}

fn info(info: &Info) -> JsValue {
    let max_message = Array::new();
    for bytes in info.max_message {
        max_message.push(&bytes.into());
    }
    object(&[
        ("panelWidth", info.panel_width.into()),
        ("panelHeight", info.panel_height.into()),
        ("orientation", info.orientation.into()),
        ("maxFps", info.max_fps.into()),
        ("maxMessage", max_message.into()),
        ("audioMaxRate", info.audio_max_rate.into()),
        ("brightness", info.brightness.into()),
        ("volume", volume(&info.volume[protocol::BULK as usize])),
        ("overlay", info.overlay_visible.into()),
    ])
}

fn event(event: &Event) -> JsValue {
    let kind = |name: &str| ("type", JsValue::from_str(name));
    match event {
        Event::Info { seq, info: i } => {
            object(&[kind("info"), ("seq", (*seq).into()), ("info", info(i))])
        }
        Event::Closed => object(&[kind("closed")]),
        Event::SwapDone { seq } => object(&[kind("swapDone"), ("seq", (*seq).into())]),
        Event::Error { seq, channel, code } => object(&[
            kind("error"),
            ("seq", (*seq).into()),
            ("channel", (*channel).into()),
            ("code", (*code).into()),
        ]),
        Event::Orientation(orientation) => {
            object(&[kind("orientation"), ("orientation", (*orientation).into())])
        }
        Event::Format { epoch, .. } => object(&[kind("format"), ("epoch", (*epoch).into())]),
        Event::Overlay { visible, .. } => {
            object(&[kind("overlay"), ("visible", (*visible).into())])
        }
        Event::Damage(_) => object(&[kind("damage")]),
        Event::Volume {
            stream,
            mute,
            volume,
        } => object(&[
            kind("volume"),
            ("stream", (*stream).into()),
            ("mute", (*mute).into()),
            ("volume", (*volume).into()),
        ]),
        Event::Audio { buffered, capacity } => object(&[
            kind("audio"),
            ("buffered", (*buffered).into()),
            ("capacity", (*capacity).into()),
        ]),
        Event::Brightness(brightness) => {
            object(&[kind("brightness"), ("brightness", (*brightness).into())])
        }
        Event::Touch(_) | Event::InputMode(..) | Event::Other => object(&[kind("other")]),
    }
}

/// The events of one control IN transfer.
#[wasm_bindgen(js_name = parseEvents)]
pub fn parse_events(data: &[u8]) -> Array {
    protocol::parse_events(data).iter().map(event).collect()
}
