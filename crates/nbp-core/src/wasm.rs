//! JavaScript bindings (`--features wasm`, built by wasm-pack). The browser
//! build drives a [`Pipeline`](crate::Pipeline) from WebUSB; both builds use
//! the encoders and parsers here, so payload layouts live only in Rust.

use crate::{frame, msg, pipeline};
use serde::Serialize;
use wasm_bindgen::prelude::*;

fn to_js<T: Serialize>(v: &T) -> JsValue {
    v.serialize(&serde_wasm_bindgen::Serializer::json_compatible()).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub struct Pipeline(pipeline::Pipeline);

#[wasm_bindgen]
impl Pipeline {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(pipeline::Pipeline::new())
    }

    pub fn push(&mut self, src: usize, bytes: &[u8]) {
        self.0.push(src, bytes);
    }

    /// [{ty, status, seq, payload: number[]}]
    pub fn take_responses(&mut self) -> JsValue {
        to_js(&self.0.take_responses())
    }

    /// New points in the flat layout of `Batch::to_f64` (a Float64Array),
    /// or undefined
    pub fn take_batch(&mut self) -> Option<Vec<f64>> {
        self.0.take_batch().map(|b| b.to_f64())
    }

    /// Meta: counters, accumulators
    pub fn meta(&self) -> JsValue {
        to_js(&self.0.meta())
    }

    /// Calibration (see units.rs); applies to new samples
    pub fn set_calibration(&mut self, cal: JsValue) -> Result<(), JsValue> {
        let c: crate::Calibration = serde_wasm_bindgen::from_value(cal)?;
        self.0.set_calibration(c);
        Ok(())
    }

    pub fn set_bucket(&mut self, n: u32) {
        self.0.set_bucket(n);
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub fn default_calibration() -> JsValue {
    to_js(&crate::Calibration::default())
}

#[wasm_bindgen]
pub fn encode(ty: u8, seq: u16, payload: &[u8]) -> Vec<u8> {
    frame::encode(ty, seq, payload)
}

#[wasm_bindgen]
pub fn parse_info(p: &[u8]) -> JsValue {
    to_js(&msg::Info::parse(p))
}

#[wasm_bindgen]
pub fn parse_stats(p: &[u8]) -> JsValue {
    to_js(&msg::Stats::parse(p))
}

#[wasm_bindgen]
pub fn parse_decim(p: &[u8]) -> JsValue {
    to_js(&msg::Decim::parse(p))
}

#[wasm_bindgen]
pub fn parse_range(p: &[u8]) -> JsValue {
    to_js(&msg::RangeState::parse(p))
}

#[wasm_bindgen]
pub fn status_name(s: u8) -> String {
    msg::status::name(s).into()
}

#[wasm_bindgen]
pub fn enc_stream_start(flags: u32) -> Vec<u8> {
    msg::enc_stream_start(flags)
}

#[wasm_bindgen]
pub fn enc_source(src: u8) -> Vec<u8> {
    msg::enc_source(src)
}

#[wasm_bindgen]
pub fn enc_decim(log2n: u8, mode: u8) -> Vec<u8> {
    msg::enc_decim(log2n, mode)
}

#[wasm_bindgen]
pub fn enc_range_set(automatic: bool, manual: u8, min: u8, max: u8) -> Vec<u8> {
    msg::enc_range_set(automatic, manual, min, max)
}

#[wasm_bindgen]
pub fn enc_reg(index: u8, value: u32) -> Vec<u8> {
    msg::enc_reg(index, value)
}
