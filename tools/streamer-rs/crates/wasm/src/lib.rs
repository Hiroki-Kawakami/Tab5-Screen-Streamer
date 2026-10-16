//! core for the Web client's workers.

mod audio;
mod media;
mod protocol;
mod video;

use js_sys::{Object, Reflect};
use wasm_bindgen::JsValue;

pub use audio::AudioConverter;
pub use media::{Media, Packet};
pub use protocol::*;
pub use video::Video;

fn object(entries: &[(&str, JsValue)]) -> JsValue {
    let object = Object::new();
    for (key, value) in entries {
        let _ = Reflect::set(&object, &JsValue::from_str(key), value);
    }
    object.into()
}
