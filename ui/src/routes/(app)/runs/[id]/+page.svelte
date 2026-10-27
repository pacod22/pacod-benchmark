<script lang="ts">
	import { onMount } from 'svelte';
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import Download from '@lucide/svelte/icons/download';
	import TimeChart from '#lib/charts/TimeChart.svelte';
	import Badge from '#lib/components/Badge.svelte';
	import Figures from '#lib/components/Figures.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime, deltaMs, duration, ms, n, pct } from '#lib/format.ts';
	import {
		cellLabel,
		flagCounts,
		latencyBars,
		liveCharts,
		overheadBars,
		resultRows,
		throughputLines
	} from '#lib/results.ts';
	import type { CellRow, CellStatus, Point, RunStatus } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const run = $derived(data.page.run);
	let status: RunStatus | null = $state(null);
	let cellStates: Record<number, CellStatus> = $state({});
	let liveIdx: number | null = $state(null);
	let points: Point[] = $state([]);
	let warnings: string[] = $state([]);

	$effect(() => {
		status = data.page.run.status;
		cellStates = {};
		warnings = [];
		liveIdx = data.live?.idx ?? null;
		points = [...(data.live?.points ?? [])];
	});

	const finished = $derived(
		['finished', 'cancelled', 'error', 'interrupted'].includes(status ?? run.status)
	);
	const cells = $derived(
		data.page.cells.map((c) => ({ ...c, status: cellStates[c.idx] ?? c.status }) as CellRow)
	);
	const rows = $derived(resultRows(cells));
	const targets = $derived(cells.filter((c) => c.kind === 'target'));
	const done = $derived(targets.filter((c) => c.status === 'done'));
	const liveCell = $derived(
		liveIdx === null ? null : (cells.find((c) => c.idx === liveIdx) ?? null)
	);
	const managed = $derived(run.target.kind === 'managed');
	const charts = $derived(
		liveCell
			? liveCharts(
					points.slice(-600),
					liveCell.config.load.mode,
					managed && liveCell.kind === 'target'
				)
			: []
	);
	const flags = $derived(flagCounts(cells));
	const m = $derived(data.page.measurement);
	const can = (p: string) => data.me.permissions.includes(p as never);
	const hasLoadAxis = $derived(done.some((c) => 'load.target' in c.axes));
	const throughput = $derived(throughputLines(cells));
	const overhead = $derived(overheadBars(cells));
	const finishedCells = $derived(
		cells.filter((c) => ['done', 'skipped', 'error', 'cancelled'].includes(c.status)).length
	);

	onMount(() => {
		if (finished) return;
		const source = new EventSource(`/runs/${run.id}/events`);
		source.addEventListener('status', (e) => {
			status = JSON.parse((e as MessageEvent).data).status;
		});
		source.addEventListener('cell', (e) => {
			const item = JSON.parse((e as MessageEvent).data);
			cellStates[item.index] = item.status;
			if (['preparing', 'running', 'draining'].includes(item.status) && liveIdx !== item.index) {
				liveIdx = item.index;
				points = [];
			}
		});
		source.addEventListener('point', (e) => {
			const item = JSON.parse((e as MessageEvent).data);
			if (item.index !== liveIdx) {
				liveIdx = item.index;
				points = [];
			}
			if (!points.some((p) => p.t === item.point.t)) points.push(item.point);
		});
		source.addEventListener('cell_done', () => void invalidate('app:run'));
		source.addEventListener('warning', (e) =>
			warnings.push(JSON.parse((e as MessageEvent).data).warning)
		);
		source.addEventListener('done', () => {
			source.close();
			void invalidate('app:run');
		});
		return () => source.close();
	});

	const totals = $derived.by(() => {
		const s = done.map((c) => c.result!.steady!);
		const calls = s.reduce((a, x) => a + x.calls, 0);
		const ok = s.reduce((a, x) => a + x.ok, 0);
		return [
			{
				label: 'Cells',
				value: `${finishedCells} of ${cells.length}`,
				note: `${done.length} target cells done`
			},
			{ label: 'Calls', value: n(calls), note: `${pct(ok, calls)} successful` },
			{
				label: 'Estimated',
				value: duration(run.plan?.estimate.duration_s),
				note: `${n(run.plan?.estimate.calls)} calls`
			},
			{
				label: 'Started',
				value: dateTime(run.started_at),
				note: run.finished_at ? `ended ${dateTime(run.finished_at)}` : 'going'
			}
		];
	});
</script>

<div class="stack-lg">
	<PageHeader
		title={run.label || `Run ${run.id.slice(-8)}`}
		description="{run.target.name} · {run.target.product.replaceAll(
			'_',
			'-'
		)} · started by {run.started_by_name}"
		crumbs={[{ label: 'Runs', href: '/runs' }]}
	>
		{#snippet actions()}
			<Badge value={status ?? run.status} />
			{#if !finished && can('bench.runs.start')}
				<form method="POST" action="?/cancel" use:enhance>
					<button class="btn btn-danger" type="submit">Cancel</button>
				</form>
			{/if}
		{/snippet}
	</PageHeader>

	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	{#if run.error}<Notice tone="error" title="The run ended with an error">{run.error}</Notice>{/if}
	{#each [...run.warnings, ...warnings] as w, i (i)}<Notice tone="warning">{w}</Notice>{/each}
	{#if run.leftovers.length}
		<Notice tone="warning" title="Left over on the gateway">
			{run.leftovers.length} objects the run made could not be deleted: {run.leftovers.join(', ')}.
			An admin cleans them up from the runs list (POST /leftovers/clean).
		</Notice>
	{/if}
	{#if m && done.length}
		<Notice
			tone={m.class === 'capacity' ? 'success' : 'info'}
			title={m.class === 'capacity'
				? 'These figures show capacity'
				: 'These figures show behaviour, not capacity'}
		>
			{#if m.reasons.length}<ul>
					{#each m.reasons as r (r)}<li>{r}</li>{/each}
				</ul>{/if}
		</Notice>
	{/if}

	<Figures figures={totals} />

	{#if liveCell && !finished}
		<section class="card stack">
			<div class="row between">
				<h2>Now: cell {liveCell.idx + 1}, {cellLabel(liveCell, true)}</h2>
				<Badge value={liveCell.status} />
			</div>
			<div class="charts">
				{#each charts as chart (chart.title)}
					<figure>
						<figcaption>{chart.title}</figcaption>
						<TimeChart
							label={chart.title}
							labels={chart.labels}
							series={chart.series}
							format={chart.unit === 'ms' ? ms : n}
							height={200}
							live
							emptyText={chart.empty}
						/>
					</figure>
				{/each}
			</div>
		</section>
	{/if}

	<section class="card stack">
		<h2>Cells</h2>
		<div class="table-wrap">
			<table>
				<thead>
					<tr>
						<th>#</th><th>Cell</th><th>Status</th><th>Calls/s</th><th>First byte p50</th><th
							>Whole p50</th
						><th>Whole p99</th><th>Gateway p99</th><th
							title="Whole call against the baseline straight to the mock">Overhead p50 / p99</th
						><th>Flags</th>
					</tr>
				</thead>
				<tbody>
					{#each rows as r (r.idx)}
						<tr class:baseline={r.kind === 'baseline'}>
							<td>{r.idx + 1}</td>
							<td><a href="/runs/{run.id}/cells/{r.idx}">{r.label}</a></td>
							<td
								><Badge value={r.status} />{#if r.error}<br /><span class="small muted"
										>{r.error}</span
									>{/if}</td
							>
							<td>{n(r.steady?.rps_mean)}</td>
							<td>{ms(r.steady?.ttfb.p50)}</td>
							<td>{ms(r.steady?.total.p50)}</td>
							<td>{ms(r.steady?.total.p99)}</td>
							<td>{r.steady ? ms(r.steady.gw_delay?.p99) : '–'}</td>
							<td
								>{r.kind === 'baseline'
									? '–'
									: `${deltaMs(r.overheadP50)} / ${deltaMs(r.overheadP99)}`}</td
							>
							<td class="small">{r.flags.map((f) => f.code).join(', ')}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="muted small">
			Steady window figures: from the end of the warm-up to the end of the last stage. The overhead
			is the whole call minus the baseline's, the same load straight to the mock; it includes what
			the gateway's own figure leaves out (relaying a stream). n/a: not measured.
		</p>
	</section>

	{#if done.length}
		<section class="card stack">
			<h2>Results</h2>
			<div class="charts">
				{#if hasLoadAxis}
					<figure>
						<figcaption>Calls a second against the load</figcaption>
						<TimeChart
							label="Calls a second against the load"
							labels={throughput.labels}
							series={throughput.series}
							format={n}
							fill={false}
							height={240}
						/>
					</figure>
				{/if}
				{#each [{ title: 'Whole call', key: 'total' }, { title: 'First byte', key: 'ttfb' }, { title: "The gateway's own delay", key: 'gw_delay' }] as const as chart (chart.key)}
					{@const b = latencyBars(cells, chart.key)}
					<figure>
						<figcaption>{chart.title}</figcaption>
						<TimeChart
							label={chart.title}
							labels={b.labels}
							series={b.series}
							format={ms}
							height={240}
							emptyText="Not measured: no answer carried the figure."
						/>
					</figure>
				{/each}
				<figure>
					<figcaption>Overhead against the baseline</figcaption>
					<TimeChart
						label="Overhead against the baseline"
						labels={overhead.labels}
						series={overhead.series}
						format={ms}
						height={240}
						emptyText="No baseline ran: set BENCH_BASELINE_MOCK_URL."
					/>
				</figure>
			</div>
		</section>
	{/if}

	{#if flags.length}
		<section class="card stack">
			<h2>What to know about these figures</h2>
			<ul class="flags">
				{#each flags as f (f.code)}<li>
						<span class="mono">{f.code}</span> ({f.cells} cell{f.cells === 1 ? '' : 's'}): {f.message}
					</li>{/each}
			</ul>
		</section>
	{/if}

	<section class="card stack">
		<h2>Fingerprint</h2>
		<dl class="kv">
			<dt>Target</dt>
			<dd>{run.target.name} ({run.target.kind})</dd>
			{#if run.observed?.status}
				<dt>Gateway version</dt>
				<dd>{run.observed.status.software_version} (observed)</dd>
				<dt>Instances</dt>
				<dd>{run.observed.status.instances?.length} (observed)</dd>
				<dt>Cache store</dt>
				<dd>{run.observed.cache?.store ?? '–'} (observed)</dd>
			{/if}
			{#each Object.entries(run.target.declared ?? {}) as [k, v] (k)}<dt>{k}</dt>
				<dd>{typeof v === 'string' ? v : JSON.stringify(v)} (declared)</dd>{/each}
			<dt>Agents</dt>
			<dd>
				{run.agents.agents.map((a) => `${a.name} (${a.placement.replace('_', ' ')})`).join(', ')}
			</dd>
		</dl>
	</section>

	{#if finished}
		<section class="card stack">
			<h2>Report</h2>
			<p class="muted">
				The run as a portable report (pacod-bench-report/1): published to pacod-gw, it shows in the
				gateway's console under Platform → Testing → Benchmarks; downloaded, it can be uploaded
				there from any other machine.
			</p>
			<div class="row">
				<a class="btn" href="/runs/{run.id}/report.json" download
					><Download size={15} /> Download the report</a
				>
				{#if can('bench.reports.publish') && data.me.publish}
					<form method="POST" action="?/publish" use:enhance>
						<button class="btn btn-primary" type="submit">Publish to pacod-gw</button>
					</form>
				{/if}
			</div>
			{#if form && 'published' in form && form.published}
				<Notice tone="success" title="Published">
					{#if form.published.link}<a href={form.published.link}>Open it in the gateway's console</a
						>{:else}Report {form.published.gw_report_id} is in the gateway's console.{/if}
				</Notice>
			{/if}
			{#if data.page.publications.length}
				<ul class="small">
					{#each data.page.publications as p (p.id)}
						<li>
							{dateTime(p.published_at)}: <Badge value={p.status} /> to {p.destination} by {p.published_by}{p.error
								? `: ${p.error}`
								: ''}
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/if}
</div>

<style>
	.between {
		justify-content: space-between;
	}

	.charts {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(22rem, 1fr));
		gap: var(--space-5);
	}

	figure {
		margin: 0;
		min-width: 0;
	}

	figcaption {
		margin-bottom: var(--space-2);
		font-weight: 600;
		font-size: var(--text-sm);
	}

	tr.baseline td {
		color: var(--text-muted);
	}

	.kv {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		gap: var(--space-2) var(--space-4);
		margin: 0;
	}

	.kv dt {
		color: var(--text-muted);
	}

	.kv dd {
		margin: 0;
	}

	.flags {
		margin: 0;
		padding-left: var(--space-5);
	}
</style>
