import { describe, expect, it } from 'vitest';
import { align, fingerprint, shared } from './compare.ts';
import { deltaMs, ms } from './format.ts';
import { cellLabel, flagCounts, latencyBars, resultRows, throughputLines } from './results.ts';
import { cleanSuite, newSuite, parseValues, presetOf, presetStages, AXES } from './suite.ts';
import type { CellRow, RunRow, Summary } from './types.ts';

const pct = (p50: number, p99: number) => ({ p50, p95: (p50 + p99) / 2, p99, max: p99 });

function summary(total: number, gw: number | null, rps = 10): Summary {
	return {
		duration_s: 10,
		started: 100,
		calls: 100,
		ok: 100,
		failed: 0,
		cancelled: 0,
		skipped: 0,
		status: { '200': 100 },
		errors: { connect: 0, timeout: 0, body: 0, other: 0, cancelled: 0 },
		bytes: 1000,
		rps_mean: rps,
		rps_peak: rps,
		ttfb: pct(total / 2, total),
		total: pct(total, total * 1.2),
		gw_delay: gw === null ? null : pct(gw, gw * 2)
	};
}

function cell(
	idx: number,
	kind: 'target' | 'baseline',
	axes: Record<string, unknown>,
	steady: Summary | null,
	key: string,
	base = 'b',
	load = 10
): CellRow {
	const config = newSuite().base;
	config.load.stages = presetStages('steady', load, 10);
	return {
		run_id: 'r',
		idx,
		rep: 0,
		kind,
		status: steady ? 'done' : 'error',
		axes,
		config,
		comparison_key: key,
		baseline_key: base,
		result: steady
			? ({ steady, flags: [], error: '', status: 'done' } as unknown as CellRow['result'])
			: null,
		started_at: null,
		finished_at: null
	};
}

describe('results', () => {
	const cells = [
		cell(0, 'baseline', {}, summary(100, null), 'b'),
		cell(1, 'target', { 'request.mode': 'managed' }, summary(125, 0.2), 'k1'),
		cell(2, 'target', { 'request.mode': 'proxy' }, summary(120, 0.1), 'k2'),
		cell(3, 'target', { 'request.mode': 'proxy' }, null, 'k3', 'missing')
	];

	it('measures every cell against its baseline', () => {
		const rows = resultRows(cells);
		expect(rows[1].overheadP50).toBe(25);
		expect(rows[2].overheadP50).toBe(20);
		expect(rows[0].overheadP50).toBeNull();
		expect(rows[3].overheadP50).toBeNull();
	});

	it('names cells by their axes', () => {
		expect(cellLabel(cells[1])).toBe('mode managed');
		expect(cellLabel(cells[0])).toBe('baseline openai 200 ms, load 10');
	});

	it('leaves an absent gateway figure absent', () => {
		const bars = latencyBars(cells, 'gw_delay');
		expect(bars.labels).toEqual(['mode managed', 'mode proxy']);
		expect(bars.series[0].values).toEqual([0.2, 0.1]);
		expect(ms(null)).toBe('n/a');
		expect(deltaMs(-2.5)).toBe('−2.50 ms');
	});

	it('draws throughput against load, one line per configuration', () => {
		const t = [
			cell(
				1,
				'target',
				{ 'request.mode': 'managed', 'load.target': 10 },
				summary(1, 0, 40),
				'a',
				'b',
				10
			),
			cell(
				2,
				'target',
				{ 'request.mode': 'managed', 'load.target': 50 },
				summary(1, 0, 180),
				'b2',
				'b',
				50
			),
			cell(
				3,
				'target',
				{ 'request.mode': 'proxy', 'load.target': 50 },
				summary(1, 0, 190),
				'c',
				'b',
				50
			)
		];
		const lines = throughputLines(t);
		expect(lines.labels).toEqual(['10', '50']);
		expect(lines.series.map((s) => s.name)).toEqual(['mode managed', 'mode proxy']);
		expect(lines.series[1].values).toEqual([null, 190]);
	});

	it('counts flags by cell', () => {
		const c = cell(1, 'target', {}, summary(1, 0), 'k');
		c.result!.flags = [{ code: 'in_stack_agent', message: 'm' }];
		expect(flagCounts([c, c])).toEqual([{ code: 'in_stack_agent', message: 'm', cells: 2 }]);
	});
});

describe('compare', () => {
	const run = (name: string, version: string, cells: CellRow[]) => ({
		run: {
			target: { name, product: 'pacod_gw', declared: { arch: name === 'a' ? 'arm64' : 'amd64' } },
			observed: { status: { software_version: version } },
			agents: { names: ['local'], agents: [{ name: 'local', url: 'u', placement: 'in_stack' }] }
		} as unknown as RunRow,
		cells
	});

	it('puts only cells of one configuration on one row', () => {
		const a = run('a', '0.1.0', [
			cell(0, 'target', { m: 1 }, summary(100, 1), 'k1'),
			cell(1, 'target', { m: 2 }, summary(100, 1), 'k2')
		]);
		const b = run('b', '0.2.0', [cell(0, 'target', { m: 1 }, summary(90, 1), 'k1')]);
		const rows = align([a, b]);
		expect(rows.map((r) => r.key)).toEqual(['k1', 'k2']);
		expect(rows[1].cells[1]).toBeNull();
		expect(shared(rows).map((r) => r.key)).toEqual(['k1']);
		const fp = fingerprint([a, b]);
		expect(fp.find((r) => r.field === 'Gateway version')).toMatchObject({
			differs: true,
			source: 'observed'
		});
		expect(fp.find((r) => r.field === 'arch')).toMatchObject({
			values: ['arm64', 'amd64'],
			source: 'declared'
		});
		expect(fp.find((r) => r.field === 'Agents')?.differs).toBe(false);
	});
});

describe('suite', () => {
	it('reads axis values by the field they set', () => {
		const load = AXES.find((a) => a.path === 'load.target');
		expect(parseValues(load, '10, 50,100')).toEqual([10, 50, 100]);
		const stream = AXES.find((a) => a.path === 'request.stream');
		expect(parseValues(stream, 'true, false')).toEqual([true, false]);
		expect(parseValues(undefined, 'a, ,b')).toEqual(['a', 'b']);
	});

	it('knows its presets again', () => {
		for (const p of ['steady', 'ramp', 'steps'] as const) {
			expect(presetOf(presetStages(p, 20, 60))).toBe(p);
		}
		expect(presetStages('steady', 5, 30)).toEqual([
			{ duration_s: 0, target: 5 },
			{ duration_s: 30, target: 5 }
		]);
	});

	it('leaves empty optional fields out', () => {
		const s = newSuite();
		s.base.key = { inspect: undefined, max_concurrency: undefined };
		s.base.load.max_calls = 0;
		s.axes['key.inspect'] = [];
		const c = cleanSuite(s);
		expect(c.base.key).toEqual({});
		expect(c.base.load.max_calls).toBeUndefined();
		expect(c.axes['key.inspect']).toBeUndefined();
	});
});
