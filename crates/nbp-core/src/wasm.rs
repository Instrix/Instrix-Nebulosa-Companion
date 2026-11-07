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

    /// [t0, t1] in `bins` columns, flat (`SampleBuffer::lod`): a Float64Array
    pub fn lod(&self, t0: f64, t1: f64, bins: usize) -> Vec<f64> {
        self.0.lod(t0, t1, bins)
    }

    /// SpanStats over [t0, t1)
    pub fn stats(&self, t0: f64, t1: f64) -> JsValue {
        to_js(&self.0.stats(t0, t1))
    }

    /// [A, V, range, math] of the sample at or before t, or null
    pub fn value_at(&self, t: f64) -> JsValue {
        to_js(&self.0.value_at(t))
    }

    /// TriggerCfg (trigger.rs)
    pub fn set_trigger(&mut self, cfg: JsValue) -> Result<(), JsValue> {
        let c: crate::trigger::TriggerCfg = serde_wasm_bindgen::from_value(cfg)?;
        self.0.set_trigger(c);
        Ok(())
    }

    /// [Event] with seq ≥ since
    pub fn events_since(&self, since: u64, max: usize) -> JsValue {
        to_js(&self.0.events_since(since, max))
    }

    pub fn clear_events(&mut self) {
        self.0.clear_events();
    }

    /// [Event] found by the script trigger
    pub fn inject_events(&mut self, events: JsValue) -> Result<(), JsValue> {
        let e: Vec<crate::trigger::Event> = serde_wasm_bindgen::from_value(events)?;
        self.0.inject_events(e);
        Ok(())
    }

    /// the samples in [t0, t1), at most max, as a script block (trigger::encode_block), in base64
    /// the bring-up's probe: raw current codes of `range` (negative: any) from now on
    pub fn probe_arm(&mut self, range: i32) {
        self.0.probe_arm(u8::try_from(range).ok());
    }

    /// [debug::ProbeResult] since probe_arm (null: not armed); disarms it
    pub fn probe_take(&mut self) -> JsValue {
        to_js(&self.0.probe_take())
    }

    pub fn sample_block(&self, t0: f64, t1: f64, max: usize) -> String {
        crate::trigger::base64(&self.0.sample_block(t0, t1, max))
    }

    /// analysis::run over [t0, t1): req is an analysis::Request
    pub fn analyze(&self, t0: f64, t1: f64, req: JsValue) -> Result<JsValue, JsValue> {
        let r: crate::analysis::Request = serde_wasm_bindgen::from_value(req)?;
        Ok(to_js(&crate::analysis::run(&self.0.extract(t0, t1), &r)))
    }

    /// a math expression (math.rs), or null for none; an error message if it doesn't compile
    pub fn set_math(&mut self, expr: Option<String>) -> Option<String> {
        self.0.set_math(expr.as_deref()).err()
    }

    /// the session log over [t0, t1] in `bins` columns (the lod layout)
    pub fn session_lod(&self, t0: f64, t1: f64, bins: usize) -> Vec<f64> {
        self.0.session_lod(t0, t1, bins)
    }

    pub fn session_stats(&self, t0: f64, t1: f64) -> JsValue {
        to_js(&self.0.session_stats(t0, t1))
    }

    pub fn set_log_interval(&mut self, interval: f64) {
        self.0.set_log_interval(interval.clamp(1e-3, 3600.0));
    }

    pub fn clear_session(&mut self) {
        self.0.clear_session();
    }

    /// What an export (export.rs ExportCfg) would write for the latest rows
    pub fn export_preview(&self, cfg: JsValue, wall_offset: f64, rows: usize) -> Result<String, JsValue> {
        let c: crate::export::ExportCfg = serde_wasm_bindgen::from_value(cfg)?;
        Ok(self.0.export_preview(&c, wall_offset, rows))
    }

    /// [t0, t1) written whole (text formats): a capture to download
    pub fn export_span(&self, cfg: JsValue, wall_offset: f64, t0: f64, t1: f64) -> Result<Vec<u8>, JsValue> {
        let c: crate::export::ExportCfg = serde_wasm_bindgen::from_value(cfg)?;
        self.0.export_span(&c, wall_offset, t0, t1).map_err(|e| JsValue::from_str(&e))
    }

    pub fn set_buffer_samples(&mut self, n: usize) {
        self.0.set_buffer_samples(n);
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
pub fn parse_board_status(p: &[u8]) -> JsValue {
    to_js(&crate::board::BoardStatus::parse(p))
}

#[wasm_bindgen]
pub fn enc_board_set(item: u8, value: u32) -> Vec<u8> {
    crate::board::enc_set(item, value)
}

#[wasm_bindgen]
pub fn enc_board_test(mask: u32) -> Vec<u8> {
    crate::board::enc_test(mask)
}

#[wasm_bindgen]
pub fn parse_update_info(p: &[u8]) -> JsValue {
    to_js(&crate::update::UpdateInfo::parse(p))
}

/// A side-channel campaign (sca.rs): traces cut out of a Pipeline's buffer.
#[wasm_bindgen]
pub struct Campaign(crate::sca::Campaign);

#[wasm_bindgen]
impl Campaign {
    /// cfg: sca::Config {samples, source}
    #[wasm_bindgen(constructor)]
    pub fn new(cfg: JsValue) -> Result<Campaign, JsValue> {
        let c: crate::sca::Config = serde_wasm_bindgen::from_value(cfg)?;
        crate::sca::Campaign::new(c).map(Self).map_err(|e| JsValue::from_str(&e))
    }

    /// the samples in [t0, t1) of `p` as one more trace; meta: sca::TraceMeta. The traces so far.
    pub fn add(&mut self, p: &Pipeline, t0: f64, t1: f64, meta: JsValue) -> Result<usize, JsValue> {
        let m: crate::sca::TraceMeta = serde_wasm_bindgen::from_value(meta)?;
        self.0.add(&p.0.extract(t0, t1), m).map_err(|e| JsValue::from_str(&e))
    }

    pub fn info(&self) -> JsValue {
        to_js(&self.0.info())
    }

    /// sca::AlignResult; throws what's wrong
    pub fn align(&mut self, a: usize, b: usize, max_shift: usize) -> Result<JsValue, JsValue> {
        self.0.align(a, b, max_shift).map(|r| to_js(&r)).map_err(|e| JsValue::from_str(&e))
    }

    pub fn clear_align(&mut self) {
        self.0.clear_align();
    }

    /// req: sca::Request; an sca::Result
    pub fn query(&self, req: JsValue) -> Result<JsValue, JsValue> {
        let r: crate::sca::Request = serde_wasm_bindgen::from_value(req)?;
        Ok(to_js(&self.0.query(&r)))
    }

    pub fn export_npz(&self) -> Vec<u8> {
        self.0.export_npz()
    }

    /// the campaign as a flat .nbsca binary
    pub fn export_raw(&self) -> Vec<u8> {
        self.0.export_raw()
    }

    /// replace this campaign with one loaded from .nbsca bytes
    pub fn load_raw(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        self.0 = crate::sca::Campaign::from_raw(bytes).map_err(|e| JsValue::from_str(&e))?;
        Ok(())
    }

    /// [sca::TraceMeta] for every trace
    pub fn meta(&self) -> JsValue {
        to_js(&self.0.meta())
    }
}

#[wasm_bindgen]
pub fn enc_update(op: u8, slot: u8, len: u32, crc: u32) -> Vec<u8> {
    crate::update::enc_update(op, slot, len, crc)
}

#[wasm_bindgen]
pub fn enc_flash(addr: u32, len: u32, data: &[u8]) -> Vec<u8> {
    crate::update::enc_flash(addr, len, data)
}

#[wasm_bindgen]
pub fn crc32(p: &[u8]) -> u32 {
    crate::update::crc32(p)
}

/// {ok, version, version_str, error}
#[wasm_bindgen]
pub fn firmware_check(img: &[u8]) -> JsValue {
    to_js(&crate::update::firmware_check(img))
}

/// the raw bitstream of a .bit / .bin; throws what's wrong
#[wasm_bindgen]
pub fn bitstream(d: &[u8]) -> Result<Vec<u8>, JsValue> {
    crate::update::bitstream(d).map_err(|e| JsValue::from_str(&e))
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

// ---- the Debug tab (debug.rs) -------------------------------------------------

#[wasm_bindgen]
pub fn reg_name(i: u8) -> String {
    crate::debug::reg_name(i).into()
}

/// REG_READ's value (undefined: a short response)
#[wasm_bindgen]
pub fn parse_reg(p: &[u8]) -> Option<u32> {
    crate::debug::parse_reg(p)
}

#[wasm_bindgen]
pub fn enc_range_timing(up_filt: u8, down_filt: u8, overlap: u8, hold: u16) -> Vec<u8> {
    crate::debug::enc_range_timing(up_filt, down_filt, overlap, hold)
}

#[wasm_bindgen]
pub fn enc_range_force(enable: bool, value: u8) -> Vec<u8> {
    crate::debug::enc_range_force(enable, value)
}

#[wasm_bindgen]
pub fn enc_range_cfg(settle: u16, align: bool, dirty_up: bool, dirty_down: bool) -> u32 {
    crate::debug::enc_range_cfg(settle, align, dirty_up, dirty_down)
}

/// [debug::RangeDecoded]
#[wasm_bindgen]
pub fn decode_range(status: u32, ctrl: u32, timing: u32, cfg: u32) -> JsValue {
    to_js(&crate::debug::decode_range(status, ctrl, timing, cfg))
}

/// [debug::AdcLink]
#[wasm_bindgen]
pub fn decode_adc(ctrl: u32, timing: u32, delay: u32, errors: u32) -> JsValue {
    to_js(&crate::debug::decode_adc(ctrl, timing, delay, errors))
}

#[wasm_bindgen]
pub fn adc_phases(a: u8, b: u8) -> u32 {
    crate::debug::adc_phases(a, b)
}

/// FLASH_READ's bytes (undefined: a short response)
#[wasm_bindgen]
pub fn parse_flash_read(p: &[u8]) -> Option<Vec<u8>> {
    crate::debug::parse_flash_read(p)
}
