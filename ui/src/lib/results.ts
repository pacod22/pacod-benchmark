/**
 * A run's cells as the result pages show them: one row a cell, the overhead against its
 * baseline, and the series of the charts. Pure, so that it is tested without a browser.
 *
 * The overhead is a cell's latency minus its baseline's: the same load straight to the mock.
 * It is what any gateway adds, pacod-gw or another product, and it covers what the gateway's own
 * figure (`x-gw-delay-ms`, the time until the response head) leaves out: the relaying of a
 * stream. A cell without a baseline that ran has no overhead (`null`), never 0.
 */
import type { ChartSeries } from '#lib/charts/types.ts';
import { clock, value } from '#lib/format.ts';
import type { CellRow, Flag, Pct, Point, Summary } from '#lib/types.ts';

/** Short names of the configuration's fields, for labels. */
const SHORT: Record<string, string> = {
	'request.mode': 'mode',
	'request.interface': 'interface',
	'request.stream': 'stream',
	'request.prompt_words': 'prompt',
	'request.max_tokens': 'answer',
	'request.http': 'http',
	'mock.family': 'mock',
	'mock.delay_ms': 'delay',
	'mock.chunk_delay_ms': 'chunk delay',
	'mock.chunks': 'chunks',
	'workspace.cache': 'cache',
	'workspace.audit': 'audit',
	'key.inspect': 'inspect',
	'key.usage_policy': 'usage',
	'load.target': 'load',
	'load.mode': 'load mode'
};

export function axisName(path: string): string {
	return SHORT[path] ?? path;
}

/** A cell in a few words: its axes, or what it is. */
export function cellLabel(
	cell: Pick<CellRow, 'kind' | 'axes' | 'rep' | 'config'>,
	withRep = false
): string {
	if (cell.kind === 'baseline') {
		const m = cell.config.mock;
		return `baseline ${m.family} ${m.delay_ms} ms, load ${peak(cell)}`;
	}
	const parts = Object.entries(cell.axes).map(([k, v]) => `${axisName(k)} ${value(v)}`);
	const text = parts.length ? parts.join(', ') : 'base';
	return withRep && cell.rep > 0 ? `${text} (#${cell.rep + 1})` : text;
}

export function peak(cell: Pick<CellRow, 'config'>): number {
	return Math.max(0, ...cell.config.load.stages.map((s) => s.target));
}

export interface ResultRow {
	idx: number;
	label: string;
	kind: 'target' | 'baseline';
	status: string;
	steady: Summary | null;
	/** Total latency minus the baseline's, at p50 and p99; `null` without a baseline. */
	overheadP50: number | null;
	overheadP99: number | null;
	ttfbOverheadP50: number | null;
	flags: Flag[];
	error: string;
}

/** Every cell's row; a target cell's overhead against the baseline it points at. */
export function resultRows(cells: CellRow[]): ResultRow[] {
	const baselines = new Map<string, Summary>();
	for (const c of cells) {
		const s = c.result?.steady;
		if (c.kind === 'baseline' && c.status === 'done' && s) baselines.set(c.comparison_key, s);
	}
	return cells.map((c) => {
		const steady = c.result?.steady ?? null;
		const base = c.kind === 'target' ? baselines.get(c.baseline_key) : undefined;
		const ok = steady && steady.ok > 0 && base && base.ok > 0;
		return {
			idx: c.idx,
			label: cellLabel(c, true),
			kind: c.kind,
			status: c.status,
			steady,
			overheadP50: ok ? steady.total.p50 - base.total.p50 : null,
			overheadP99: ok ? steady.total.p99 - base.total.p99 : null,
			ttfbOverheadP50: ok ? steady.ttfb.p50 - base.ttfb.p50 : null,
			flags: c.result?.flags ?? [],
			error: c.result?.error ?? ''
		};
	});
}

/** Latency of each finished target cell as grouped columns: p50, p95, p99. */
export function latencyBars(
	cells: CellRow[],
	which: 'total' | 'ttfb' | 'gw_delay'
): { labels: string[]; series: ChartSeries[] } {
	const done = cells.filter((c) => c.kind === 'target' && c.result?.steady && c.status === 'done');
	const pick = (c: CellRow, q: 'p50' | 'p95' | 'p99') => {
		const s = c.result?.steady;
		const p = s ? (which === 'gw_delay' ? s.gw_delay : s[which]) : null;
		return p ? p[q] : null;
	};
	return {
		labels: done.map((c) => cellLabel(c, true)),
		series: [
			{ name: 'p50', values: done.map((c) => pick(c, 'p50')), colour: '--series-1', bar: true },
			{ name: 'p95', values: done.map((c) => pick(c, 'p95')), colour: '--series-3', bar: true },
			{ name: 'p99', values: done.map((c) => pick(c, 'p99')), colour: '--series-5', bar: true }
		]
	};
}

/** The overhead against the baseline of each target cell, p50 and p99. */
export function overheadBars(cells: CellRow[]): { labels: string[]; series: ChartSeries[] } {
	const rows = resultRows(cells).filter((r) => r.kind === 'target' && r.status === 'done');
	return {
		labels: rows.map((r) => r.label),
		series: [
			{ name: 'p50', values: rows.map((r) => r.overheadP50), colour: '--series-1', bar: true },
			{ name: 'p99', values: rows.map((r) => r.overheadP99), colour: '--series-5', bar: true }
		]
	};
}

/**
 * Calls a second against the offered load: one line per configuration (the axes without
 * `load.target`), the load on the horizontal axis. Shows where a configuration stops keeping up.
 */
export function throughputLines(cells: CellRow[]): { labels: string[]; series: ChartSeries[] } {
	const done = cells.filter((c) => c.kind === 'target' && c.status === 'done' && c.result?.steady);
	const loads = [...new Set(done.map(peak))].sort((a, b) => a - b);
	const groups = new Map<string, Map<number, number[]>>();
	for (const c of done) {
		const rest = Object.entries(c.axes)
			.filter(([k]) => k !== 'load.target')
			.map(([k, v]) => `${axisName(k)} ${value(v)}`)
			.join(', ');
		const name = rest || 'base';
		const byLoad = groups.get(name) ?? new Map<number, number[]>();
		const list = byLoad.get(peak(c)) ?? [];
		list.push(c.result!.steady!.rps_mean);
		byLoad.set(peak(c), list);
		groups.set(name, byLoad);
	}
	const colours = [
		'--series-1',
		'--series-2',
		'--series-3',
		'--series-4',
		'--series-5',
		'--series-6'
	];
	const series = [...groups.entries()].map(([name, byLoad], i) => ({
		name,
		values: loads.map((l) => {
			const v = byLoad.get(l);
			// Repetitions are averaged.
			return v ? v.reduce((a, b) => a + b, 0) / v.length : null;
		}),
		colour: colours[i % colours.length]
	}));
	return { labels: loads.map((l) => String(l)), series };
}

/** The codes of every flag of the run's cells, each once, with how many cells carry it. */
export function flagCounts(cells: CellRow[]): { code: string; message: string; cells: number }[] {
	const out = new Map<string, { code: string; message: string; cells: number }>();
	for (const c of cells) {
		for (const f of c.result?.flags ?? []) {
			const e = out.get(f.code) ?? { code: f.code, message: f.message, cells: 0 };
			e.cells += 1;
			out.set(f.code, e);
		}
	}
	return [...out.values()];
}

export interface LiveChart {
	title: string;
	labels: string[];
	series: ChartSeries[];
	unit: 'calls' | 'ms';
	empty: string;
}

/** The charts of one cell's seconds, as pacod-gw's load page draws them. */
export function liveCharts(
	points: Point[],
	mode: 'concurrency' | 'rate',
	gateway: boolean
): LiveChart[] {
	const labels = points.map((p) => clock(p.t));
	const pick = (p: Pct | null, q: 'p50' | 'p95' | 'p99') => (p ? p[q] : null);
	const latency = (
		title: string,
		key: 'ttfb' | 'total' | 'gw_delay',
		empty: string
	): LiveChart => ({
		title,
		labels,
		unit: 'ms',
		empty,
		series: [
			{ name: 'p50', values: points.map((p) => pick(p[key], 'p50')), colour: '--series-1' },
			{ name: 'p95', values: points.map((p) => pick(p[key], 'p95')), colour: '--series-3' },
			{ name: 'p99', values: points.map((p) => pick(p[key], 'p99')), colour: '--series-5' }
		]
	});
	return [
		{
			title: 'Calls a second',
			labels,
			unit: 'calls',
			empty: 'No call has finished yet.',
			series: [
				{ name: 'Successful', values: points.map((p) => p.ok), colour: '--series-1' },
				...(mode === 'rate'
					? [{ name: 'Target', values: points.map((p) => p.target), colour: '--series-4' }]
					: []),
				{ name: 'Failed', values: points.map((p) => p.failed), colour: '--danger', bar: true },
				{
					name: 'Not started',
					values: points.map((p) => p.skipped),
					colour: '--warning',
					bar: true
				}
			]
		},
		latency('Whole call', 'total', 'No successful call yet.'),
		latency('Time to the first byte', 'ttfb', 'No successful call yet.'),
		latency(
			"The gateway's own delay",
			'gw_delay',
			'The target sends no x-gw-delay-ms: it is not pacod-gw, or this is a baseline.'
		),
		{
			title: 'Calls in flight',
			labels,
			unit: 'calls',
			empty: 'Nothing in flight yet.',
			series: [
				{ name: 'At the agents', values: points.map((p) => p.in_flight), colour: '--series-1' },
				...(gateway
					? [
							{
								name: 'At the gateway',
								values: points.map((p) => p.gw_in_flight),
								colour: '--series-6'
							}
						]
					: [])
			]
		}
	];
}
