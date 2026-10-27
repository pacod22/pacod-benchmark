<script lang="ts">
	import TimeChart from '#lib/charts/TimeChart.svelte';
	import Badge from '#lib/components/Badge.svelte';
	import Figures from '#lib/components/Figures.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { deltaMs, ms, n, pct } from '#lib/format.ts';
	import { cellLabel, liveCharts } from '#lib/results.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const c = $derived(data.cell);
	const r = $derived(c.result);
	const s = $derived(r?.steady ?? null);
	const w = $derived(r?.summary ?? null);
	const base = $derived(data.baseline?.result?.steady ?? null);
	const charts = $derived(
		liveCharts(
			c.points ?? [],
			c.config.load.mode,
			data.run.target.kind === 'managed' && c.kind === 'target'
		)
	);
	const figures = $derived(
		s
			? [
					{
						label: 'Calls',
						value: n(s.calls),
						note: `${pct(s.ok, s.calls)} successful${s.skipped ? `, ${n(s.skipped)} not started` : ''}`
					},
					{
						label: 'A second',
						value: n(s.rps_mean),
						note: `${n(s.rps_peak)} in the busiest second`
					},
					{ label: 'First byte', value: ms(s.ttfb.p50), note: `p99 ${ms(s.ttfb.p99)}` },
					{
						label: 'Whole call',
						value: ms(s.total.p50),
						note: `p95 ${ms(s.total.p95)}, p99 ${ms(s.total.p99)}`
					},
					{
						label: 'Gateway delay',
						value: s.gw_delay ? ms(s.gw_delay.p50) : 'n/a',
						note: s.gw_delay ? `p99 ${ms(s.gw_delay.p99)}` : 'not reported by the target'
					},
					{
						label: 'Overhead',
						value: base ? deltaMs(s.total.p50 - base.total.p50) : 'n/a',
						note: base
							? `p99 ${deltaMs(s.total.p99 - base.total.p99)} against the baseline`
							: 'no baseline ran'
					}
				]
			: []
	);
</script>

<div class="stack-lg">
	<PageHeader
		title="Cell {c.idx + 1}: {cellLabel(c, true)}"
		description={c.kind === 'baseline'
			? 'A baseline: the same load straight to the mock, without any gateway.'
			: `On ${data.run.target.name}.`}
		crumbs={[
			{ label: 'Runs', href: '/runs' },
			{ label: data.run.label || data.run.id.slice(-8), href: `/runs/${data.run.id}` }
		]}
	>
		{#snippet actions()}<Badge value={c.status} />{/snippet}
	</PageHeader>
	{#if r?.error}<Notice tone="error">{r.error}</Notice>{/if}
	{#if s}<Figures {figures} />{/if}
	{#if w && s}
		<p class="muted small">
			Steady window: {s.duration_s} s of {w.duration_s} s ({n(w.calls)} calls in all, whole-call p99 {ms(
				w.total.p99
			)} over the whole cell).
		</p>
	{/if}
	{#each r?.flags ?? [] as f (f.code)}<Notice tone="warning"
			>{f.message} <span class="mono small">({f.code})</span></Notice
		>{/each}

	<section class="card stack">
		<h2>Second by second</h2>
		<div class="charts">
			{#each charts as chart (chart.title)}
				<figure>
					<figcaption>{chart.title}</figcaption>
					<TimeChart
						label={chart.title}
						labels={chart.labels}
						series={chart.series}
						format={chart.unit === 'ms' ? ms : n}
						height={220}
						emptyText={chart.empty}
					/>
				</figure>
			{/each}
		</div>
	</section>

	<div class="grid">
		<section class="card stack">
			<h2>Configuration</h2>
			<pre class="small">{JSON.stringify(c.config, null, 2)}</pre>
		</section>
		<section class="card stack">
			<h2>Agents</h2>
			<div class="table-wrap">
				<table>
					<thead
						><tr
							><th>Agent</th><th>Where</th><th>State</th><th>Calls</th><th>CPU at most</th><th
								>Lag at most</th
							><th>Probe</th></tr
						></thead
					>
					<tbody>
						{#each r?.agents ?? [] as a (a.name)}
							<tr>
								<td class="mono">{a.name}</td>
								<td>{a.placement.replace('_', ' ')}</td>
								<td
									><Badge value={a.state} />{#if a.error}<br /><span class="small muted"
											>{a.error}</span
										>{/if}</td
								>
								<td>{n(a.calls)}</td>
								<td>{a.cpu_max.toFixed(0)}% of {a.cpus}</td>
								<td>{ms(a.lag_max_ms)}</td>
								<td class="small"
									>{a.probe ? `${a.probe.status ?? '–'} in ${ms(a.probe.total_ms)}` : '–'}</td
								>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
			{#if r?.noise}
				<dl class="kv small">
					<dt>Quiet before</dt>
					<dd>
						{r.noise.quiet_before === null
							? 'not checked'
							: r.noise.quiet_before
								? 'yes'
								: 'no: other calls were in flight'}
					</dd>
					<dt>Calls beyond the cell's at the gateway</dt>
					<dd>{r.noise.excess_in_flight ?? '–'}</dd>
					<dt>Configuration version</dt>
					<dd>{r.noise.config_head_before ?? '–'} → {r.noise.config_head_after ?? '–'}</dd>
					<dt>Key</dt>
					<dd class="mono">{r.key_hint || '–'}</dd>
				</dl>
			{/if}
		</section>
	</div>
</div>

<style>
	.charts,
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(22rem, 1fr));
		gap: var(--space-5);
		align-items: start;
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

	pre {
		margin: 0;
		white-space: pre-wrap;
	}

	.kv {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		gap: var(--space-1) var(--space-4);
		margin: 0;
	}

	.kv dt {
		color: var(--text-muted);
	}

	.kv dd {
		margin: 0;
	}
</style>
