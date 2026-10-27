/**
 * Runs side by side. Only cells with the same comparison key ran the same configuration (the
 * request, the mock behind the target, the load, the gateway's settings), so only they are put
 * on one row; a cell that no other run has stands alone. Deployment-level differences (the
 * target, its version, its layout, where the agents ran) are listed apart: they are what a
 * comparison across targets or platforms compares.
 */
import { cellLabel, resultRows, type ResultRow } from '#lib/results.ts';
import type { CellRow, RunRow } from '#lib/types.ts';

export interface ComparedRun {
	run: RunRow;
	cells: CellRow[];
}

export interface AlignedRow {
	key: string;
	label: string;
	kind: 'target' | 'baseline';
	/** One entry per run, in the order of the runs; `null` where the run has no such cell. */
	cells: (ResultRow | null)[];
}

/** The rows of the comparison: keys in the order the first run that has them ran them. */
export function align(runs: ComparedRun[]): AlignedRow[] {
	const rows = new Map<string, AlignedRow>();
	runs.forEach((r, i) => {
		const results = resultRows(r.cells);
		r.cells.forEach((c, j) => {
			// Repetitions of one configuration: the first that ran stands for them.
			let row = rows.get(c.comparison_key);
			if (!row) {
				row = {
					key: c.comparison_key,
					label: cellLabel(c),
					kind: c.kind,
					cells: runs.map(() => null)
				};
				rows.set(c.comparison_key, row);
			}
			if (!row.cells[i] || row.cells[i]!.status !== 'done') row.cells[i] = results[j];
		});
	});
	return [...rows.values()];
}

/** Rows that at least two runs have: the ones a comparison is about. */
export function shared(rows: AlignedRow[]): AlignedRow[] {
	return rows.filter((r) => r.cells.filter((c) => c && c.status === 'done').length >= 2);
}

export interface FingerprintRow {
	field: string;
	values: string[];
	/** The runs differ here. */
	differs: boolean;
	/** `observed` when the gateway said it, `declared` when the operator did. */
	source: 'observed' | 'declared' | 'run';
}

const text = (v: unknown): string =>
	v === undefined || v === null || v === '' ? '–' : typeof v === 'string' ? v : JSON.stringify(v);

/** What each run ran against, field by field. */
export function fingerprint(runs: ComparedRun[]): FingerprintRow[] {
	const rows: FingerprintRow[] = [];
	const add = (
		field: string,
		source: FingerprintRow['source'],
		get: (r: ComparedRun) => unknown
	) => {
		const values = runs.map((r) => text(get(r)));
		rows.push({ field, values, differs: new Set(values).size > 1, source });
	};
	add('Target', 'run', (r) => r.run.target.name);
	add('Product', 'run', (r) => r.run.target.product);
	add('Gateway version', 'observed', (r) => r.run.observed?.status?.software_version);
	add('Instances', 'observed', (r) => r.run.observed?.status?.instances?.length);
	add('Cache store', 'observed', (r) => r.run.observed?.cache?.store);
	add(
		'Analytics',
		'observed',
		(r) => r.run.observed?.status?.components?.find((c) => c.name === 'analytics')?.status
	);
	const declared = new Set(runs.flatMap((r) => Object.keys(r.run.target.declared ?? {})));
	for (const key of [...declared].sort()) add(key, 'declared', (r) => r.run.target.declared?.[key]);
	add('Agents', 'run', (r) =>
		r.run.agents.agents.map((a) => `${a.name} (${a.placement.replace('_', ' ')})`).join(', ')
	);
	return rows;
}
