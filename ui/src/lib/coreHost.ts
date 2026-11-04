// The receive pipeline (Rust core, WASM) and what works on its samples: the
// side-channel campaign and, for the demo, the signal generator. It answers
// named calls; coreClient.ts runs it in a Web Worker, so parsing the stream,
// keeping the buffer and answering queries never take the UI thread's time
// (or, where no worker can start, in the page as before).

import { core, type Meta } from './nbp';
import { Generator, RATE, TICK_MS, type DemoScenario, type OpSpec } from './demoGen';

/** what the host tells its client unasked */
export type HostEvent =
  | { type: 'meta'; meta: Meta }
  | { type: 'responses'; list: { ty: number; status: number; seq: number; payload: number[] }[] };

type Emit = (e: HostEvent) => void;

/** the pipeline methods callable by name (they take and return plain data) */
const PIPELINE = new Set([
  'lod',
  'stats',
  'value_at',
  'set_trigger',
  'events_since',
  'clear_events',
  'analyze',
  'set_math',
  'session_lod',
  'session_stats',
  'set_log_interval',
  'clear_session',
  'export_preview',
  'export_span',
  'set_buffer_samples',
  'set_calibration',
  'meta',
]);

export class CoreHost {
  private pipeline = new core.Pipeline();
  private campaign: core.Campaign | null = null;
  private gen: Generator | null = null;
  private streaming = true;
  private timers: ReturnType<typeof setInterval>[] = [];
  private sent = -1;

  constructor(private emit: Emit) {}

  /** a new Meta if the pipeline changed since the last */
  private sendMeta() {
    const m = this.pipeline.meta() as Meta;
    if (m.version !== this.sent) {
      this.sent = m.version;
      this.emit({ type: 'meta', meta: m });
    }
  }

  private campaignOrThrow(): core.Campaign {
    if (!this.campaign) throw new Error('no campaign: create one first');
    return this.campaign;
  }

  handle(method: string, args: unknown[]): unknown {
    if (PIPELINE.has(method)) {
      const p = this.pipeline as unknown as Record<string, (...a: unknown[]) => unknown>;
      return p[method](...args);
    }
    switch (method) {
      case 'ping':
        return 'pong';
      // a transport's bytes; responses to commands go straight back
      case 'push': {
        this.pipeline.push(args[0] as number, args[1] as Uint8Array);
        const list = this.pipeline.take_responses() as Extract<HostEvent, { type: 'responses' }>['list'];
        if (list.length) this.emit({ type: 'responses', list });
        return;
      }
      // a Meta every `ms` while something changed
      case 'watch': {
        this.timers.push(setInterval(() => this.sendMeta(), args[0] as number));
        return;
      }
      // the demo device: the generator feeds the pipeline every tick
      case 'demo.start': {
        this.gen ??= new Generator();
        const gen = this.gen;
        this.timers.push(
          setInterval(() => {
            if (!this.streaming) return;
            for (const m of gen.next((RATE * TICK_MS) / 1000)) this.pipeline.push(0, m);
            this.sendMeta();
          }, TICK_MS),
        );
        return;
      }
      case 'demo.stream':
        this.streaming = !!args[0];
        return;
      case 'demo.scenario':
        if (this.gen) this.gen.scenario = args[0] as DemoScenario;
        return;
      // resolves when the target has done it (in generator time)
      case 'demo.schedule':
        return this.gen?.schedule(args[0] as number, args[1] as OpSpec);
      // the side-channel campaign: over this pipeline's samples
      case 'sca.create':
        this.campaign?.free();
        this.campaign = new core.Campaign(args[0]);
        return;
      case 'sca.add':
        return this.campaignOrThrow().add(this.pipeline, args[0] as number, args[1] as number, args[2]);
      case 'sca.info':
        return this.campaign ? this.campaign.info() : null;
      case 'sca.align':
        return this.campaignOrThrow().align(args[0] as number, args[1] as number, args[2] as number);
      case 'sca.clear_align':
        this.campaign?.clear_align();
        return;
      case 'sca.query':
        return this.campaignOrThrow().query(args[0]);
      case 'sca.export_npz':
        return this.campaignOrThrow().export_npz();
      case 'sca.meta':
        return this.campaign ? this.campaign.meta() : [];
      case 'sca.export_raw':
        return this.campaignOrThrow().export_raw();
      case 'sca.load_raw': {
        this.campaign?.free();
        this.campaign = new core.Campaign({ samples: 2, source: 'i' });
        this.campaign.load_raw(args[0] as Uint8Array);
        return this.campaign.info();
      }
      // a new link: a fresh pipeline (and generator); the campaign stays (traces from any connection)
      case 'reset':
        for (const t of this.timers) clearInterval(t);
        this.timers = [];
        this.pipeline.free();
        this.pipeline = new core.Pipeline();
        this.gen = null;
        this.streaming = true;
        this.sent = -1;
        return;
      case 'close':
        for (const t of this.timers) clearInterval(t);
        this.timers = [];
        return;
    }
    throw new Error(`core host: unknown call '${method}'`);
  }
}
