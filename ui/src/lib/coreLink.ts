// What every link whose pipeline is the WASM core has in common (WebUSB, the
// demo): the buffer, session log, trigger and analysis queries, all answered
// by a core host (coreClient.ts), in a worker when one starts.

import type { LinkEvents } from './device';
import type { AlignResult, AnalysisRequest, AnalysisResult, Calibration, ExportCfg, RawSpanStats, ScaInfo, ScaResult, TraceMeta, TrigEvent, TriggerCfg, ProbeResult } from './nbp';
import type { ScaEngine } from './sca';
import { openCore, type CoreClient } from './coreClient';
import type { HostEvent } from './coreHost';

export abstract class CoreLink {
  /** the core host: starting (its WASM loads) while the link opens */
  protected core: Promise<CoreClient>;

  constructor(protected ev: LinkEvents) {
    this.core = openCore(this, (e) => this.onHost(e));
  }

  /** an event from the host: Meta to the UI, responses to the transport */
  protected onHost(e: HostEvent) {
    if (e.type === 'meta') this.ev.onMeta(e.meta);
  }

  protected async call<T>(method: string, ...args: unknown[]): Promise<T> {
    return (await this.core).call<T>(method, ...args);
  }

  async setBuffer(samples: number) {
    await this.call('set_buffer_samples', samples);
  }
  lod(t0: number, t1: number, bins: number) {
    return this.call<Float64Array>('lod', t0, t1, bins);
  }
  stats(t0: number, t1: number) {
    return this.call<RawSpanStats | null>('stats', t0, t1);
  }
  valueAt(t: number) {
    return this.call<[number, number, number, number] | null>('value_at', t);
  }
  sessionLod(t0: number, t1: number, bins: number) {
    return this.call<Float64Array>('session_lod', t0, t1, bins);
  }
  sessionStats(t0: number, t1: number) {
    return this.call<RawSpanStats | null>('session_stats', t0, t1);
  }
  async setLogInterval(s: number) {
    await this.call('set_log_interval', s);
  }
  async clearSession() {
    await this.call('clear_session');
  }
  exportPreview(cfg: ExportCfg, wallOffset: number, rows: number) {
    return this.call<string>('export_preview', cfg, wallOffset, rows);
  }
  exportSpan(cfg: ExportCfg, wallOffset: number, t0: number, t1: number) {
    return this.call<Uint8Array>('export_span', cfg, wallOffset, t0, t1);
  }
  async setMath(expr: string | null) {
    return (await this.call<string | undefined>('set_math', expr ?? undefined)) ?? null;
  }
  async setCalibration(c: Calibration) {
    await this.call('set_calibration', c);
  }
  async setTrigger(c: TriggerCfg) {
    await this.call('set_trigger', c);
  }
  events(since: number, max: number) {
    return this.call<TrigEvent[]>('events_since', BigInt(since), max);
  }
  async injectEvents(e: TrigEvent[]) {
    await this.call('inject_events', e);
  }
  async probeArm(range: number) {
    await this.call('probe_arm', range);
  }
  probeTake() {
    return this.call<ProbeResult | null>('probe_take').then((r) => r ?? null);
  }
  sampleBlock(t0: number, t1: number, max: number) {
    return this.call<string>('sample_block', t0, t1, max);
  }
  async clearEvents() {
    await this.call('clear_events');
  }
  analyze(t0: number, t1: number, req: AnalysisRequest) {
    return this.call<AnalysisResult>('analyze', t0, t1, req);
  }

  /** side-channel campaigns: in the host, beside the samples they cut traces from */
  readonly sca: ScaEngine = {
    create: async (cfg) => void (await this.call('sca.create', cfg)),
    add: (t0, t1, meta) => this.call<number>('sca.add', t0, t1, meta),
    info: () => this.call<ScaInfo | null>('sca.info'),
    align: (a, b, maxShift) => this.call<AlignResult>('sca.align', a, b, maxShift),
    clearAlign: async () => void (await this.call('sca.clear_align')),
    query: (req) => this.call<ScaResult>('sca.query', req),
    exportNpz: () => this.call<Uint8Array>('sca.export_npz'),
    meta: () => this.call<TraceMeta[]>('sca.meta'),
    save: () => this.call<Uint8Array>('sca.export_raw'),
    load: (bytes) => this.call<ScaInfo | null>('sca.load_raw', bytes),
  };

  /** done with the host: its timers stop (the worker and the campaign stay for the next link) */
  protected async closeCore() {
    (await this.core.catch(() => null))?.release(this);
  }
}
