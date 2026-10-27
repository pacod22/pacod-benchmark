/**
 * The suite editor's model: a suite as the form edits it, the axes it offers, and the load
 * presets. The API checks the suite; this only shapes it.
 */
import type { CellConfig, Stage, SuiteDocument } from '#lib/types.ts';

export interface AxisChoice {
	path: string;
	label: string;
	/** The values the field takes, or a kind of value for free input. */
	options?: string[];
	kind: 'enum' | 'number' | 'bool';
	hint: string;
}

/** Every field a suite can vary, with what it means. */
export const AXES: AxisChoice[] = [
	{
		path: 'request.mode',
		label: 'Mode',
		kind: 'enum',
		options: ['managed', 'proxy'],
		hint: 'Planned and possibly translated, or relayed unchanged.'
	},
	{
		path: 'request.interface',
		label: 'Interface',
		kind: 'enum',
		options: ['openai', 'anthropic', 'gemini', 'native', 'embeddings'],
		hint: 'The API the calls are made in. With another mock family than its own, managed mode translates.'
	},
	{
		path: 'request.stream',
		label: 'Streamed',
		kind: 'bool',
		hint: 'Streamed answers, or whole ones.'
	},
	{
		path: 'request.prompt_words',
		label: 'Prompt words',
		kind: 'number',
		hint: 'How long the prompt is; the mock answers with about as much.'
	},
	{
		path: 'request.max_tokens',
		label: 'Answer tokens',
		kind: 'number',
		hint: 'max_tokens of each call.'
	},
	{
		path: 'request.http',
		label: 'HTTP',
		kind: 'enum',
		options: ['1.1', '2'],
		hint: 'HTTP/1.1: one call per connection. HTTP/2: many calls over a few.'
	},
	{
		path: 'mock.family',
		label: 'Mock family',
		kind: 'enum',
		options: ['openai', 'anthropic', 'gemini'],
		hint: 'The dialect the gateway speaks upstream.'
	},
	{
		path: 'mock.delay_ms',
		label: 'Mock delay',
		kind: 'number',
		hint: 'Milliseconds before the mock answers.'
	},
	{
		path: 'mock.chunk_delay_ms',
		label: 'Chunk delay',
		kind: 'number',
		hint: 'Milliseconds between stream chunks.'
	},
	{ path: 'mock.chunks', label: 'Chunks', kind: 'number', hint: 'Stream chunks of each answer.' },
	{
		path: 'workspace.cache',
		label: 'Response cache',
		kind: 'enum',
		options: ['off', 'shadow', 'enforce'],
		hint: 'The workspace binding of the response cache (managed mode only).'
	},
	{
		path: 'workspace.audit',
		label: 'Audit mode',
		kind: 'enum',
		options: ['off', 'on', 'on_request'],
		hint: 'Whether calls are recorded in full.'
	},
	{
		path: 'key.inspect',
		label: 'Inspect',
		kind: 'enum',
		options: ['full', 'none'],
		hint: 'Whether proxy mode reads the request body (none: blind relay).'
	},
	{
		path: 'key.usage_policy',
		label: 'Usage policy',
		kind: 'enum',
		options: ['require', 'estimate', 'ignore'],
		hint: 'What the gateway does without usage in an answer.'
	},
	{
		path: 'load.target',
		label: 'Load',
		kind: 'number',
		hint: 'The peak: calls at a time, or a second. Every stage is scaled.'
	},
	{
		path: 'load.mode',
		label: 'Load mode',
		kind: 'enum',
		options: ['concurrency', 'rate'],
		hint: 'A closed loop of callers, or an open loop of arrivals (needs max in flight).'
	}
];

/** The values of an axis as the form gives them: comma-separated text. */
export function parseValues(axis: AxisChoice | undefined, text: string): unknown[] {
	const parts = text
		.split(',')
		.map((s) => s.trim())
		.filter((s) => s !== '');
	return parts.map((p) => {
		if (axis?.kind === 'number') {
			const n = Number(p);
			return Number.isFinite(n) ? n : p;
		}
		if (axis?.kind === 'bool') return p === 'true' || p === 'yes' || p === 'on';
		return p;
	});
}

export function formatValues(values: unknown[]): string {
	return values.map((v) => (typeof v === 'string' ? v : JSON.stringify(v))).join(', ');
}

export type Preset = 'steady' | 'ramp' | 'steps';

/** The stages of a preset, as pacod-gw's load form offers them. */
export function presetStages(preset: Preset, target: number, seconds: number): Stage[] {
	switch (preset) {
		case 'steady':
			return [
				{ duration_s: 0, target },
				{ duration_s: seconds, target }
			];
		case 'ramp': {
			const up = Math.max(1, Math.round(seconds / 3));
			return [
				{ duration_s: up, target },
				{ duration_s: Math.max(1, seconds - 2 * up), target },
				{ duration_s: up, target: 0 }
			];
		}
		case 'steps': {
			const step = Math.max(1, Math.round(seconds / 4));
			return [1, 2, 3, 4].flatMap((k) => [
				{ duration_s: 0, target: Math.round((target * k) / 4) },
				{ duration_s: step, target: Math.round((target * k) / 4) }
			]);
		}
	}
}

/** Which preset a list of stages is, if any. */
export function presetOf(stages: Stage[]): Preset | null {
	if (stages.length === 2 && stages[0].duration_s === 0 && stages[0].target === stages[1].target)
		return 'steady';
	if (stages.length === 3 && stages[2].target === 0 && stages[0].target === stages[1].target)
		return 'ramp';
	if (stages.length === 8) return 'steps';
	return null;
}

export function baseConfig(): CellConfig {
	return {
		request: {
			interface: 'openai',
			mode: 'managed',
			stream: true,
			prompt_words: 60,
			max_tokens: 64,
			http: '1.1'
		},
		mock: { family: 'openai', delay_ms: 200, chunk_delay_ms: 20, chunks: 20 },
		workspace: { cache: 'off', audit: 'on_request' },
		key: {},
		load: {
			mode: 'concurrency',
			stages: presetStages('steady', 20, 60),
			timeout_s: 120,
			warmup_s: 5
		}
	};
}

/** A new suite: managed against proxy mode at three load levels. */
export function newSuite(): SuiteDocument {
	return {
		name: 'Managed against proxy mode',
		description: 'The same calls in both modes at three load levels, behind the same mock.',
		base: baseConfig(),
		axes: { 'request.mode': ['managed', 'proxy'], 'load.target': [10, 50, 100] },
		order: { kind: 'grouped' },
		repetitions: 1,
		cap: 64,
		baseline: true
	};
}

/** Ready-made suites to start from. */
export const TEMPLATES: { id: string; title: string; make: () => SuiteDocument }[] = [
	{ id: 'modes', title: 'Managed against proxy mode, three load levels', make: newSuite },
	{
		id: 'latency',
		title: 'What the gateway adds at three provider latencies',
		make: () => ({
			...newSuite(),
			name: 'Gateway overhead by provider latency',
			description:
				'The mock answering in 20, 200 and 1000 ms: the overhead should not grow with it.',
			axes: { 'mock.delay_ms': [20, 200, 1000], 'request.mode': ['managed', 'proxy'] }
		})
	},
	{
		id: 'translation',
		title: 'Same dialect against translated',
		make: () => ({
			...newSuite(),
			name: 'Translation cost',
			description:
				'OpenAI calls to the OpenAI mock (patched in place) and to the Anthropic and Gemini mocks (translated).',
			axes: { 'mock.family': ['openai', 'anthropic', 'gemini'], 'request.stream': [true, false] }
		})
	},
	{
		id: 'cache',
		title: 'The response cache off and on',
		make: () => ({
			...newSuite(),
			name: 'Response cache',
			description:
				'Every call sends the same body: with the cache on, nearly every call is answered from it.',
			axes: { 'workspace.cache': ['off', 'enforce'], 'load.target': [10, 50] }
		})
	},
	{
		id: 'external',
		title: 'One configuration for an external target',
		make: () => ({
			...newSuite(),
			name: 'External target',
			description: 'Load levels only: the benchmark sets nothing up on an external target.',
			axes: { 'load.target': [5, 20, 50] }
		})
	}
];

/** The suite as the API takes it: optional fields left empty are left out. */
export function cleanSuite(doc: SuiteDocument): SuiteDocument {
	const copy: SuiteDocument = JSON.parse(JSON.stringify(doc));
	const key = copy.base.key;
	for (const k of Object.keys(key) as (keyof typeof key)[]) {
		if (key[k] === undefined || key[k] === null || (key[k] as unknown) === '') delete key[k];
	}
	const ws = copy.base.workspace;
	if (!ws.responses_store) delete ws.responses_store;
	if (!ws.on_spool_full) delete ws.on_spool_full;
	const load = copy.base.load;
	if (!load.max_in_flight) delete load.max_in_flight;
	if (!load.max_calls) delete load.max_calls;
	if (copy.base.request.headers && Object.keys(copy.base.request.headers).length === 0)
		delete copy.base.request.headers;
	for (const [path, values] of Object.entries(copy.axes))
		if (values.length === 0) delete copy.axes[path];
	return copy;
}
