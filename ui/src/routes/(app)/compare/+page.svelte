<script lang="ts">
	import TimeChart from '#lib/charts/TimeChart.svelte';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { align, fingerprint, shared } from '#lib/compare.ts';
	import { dateTime, deltaMs, ms, n } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	// The runs of the address at first, then as ticked.
	let chosen: string[] = $derived([...data.chosen]);
	const compared = $derived(data.compared ?? []);
	const rows = $derived(align(compared));
	const common = $derived(shared(rows).filter((r) => r.kind === 'target'));
	const fp = $derived(fingerprint(compared));
	const names = $derived(
		compared.map((r) => r.run.label || `${r.run.target.name} ${r.run.id.slice(-6)}`)
	);
	const colours = [
		'--series-1',
		'--series-2',
		'--series-3',
		'--series-4',
		'--series-5',
		'--series-6'
	];
	const bars = (pick: (i: number, row: (typeof common)[number]) => number | null) => ({
		labels: common.map((r) => r.label),
		series: compared.map((_, i) => ({
			name: names[i],
			values: common.map((row) => pick(i, row)),
			colour: colours[i % 6],
			bar: true
		}))
	});
	const p99 = $derived(bars((i, row) => row.cells[i]?.steady?.total.p99 ?? null));
	const overhead = $derived(bars((i, row) => row.cells[i]?.overheadP50 ?? null));
	const rps = $derived(bars((i, row) => row.cells[i]?.steady?.rps_mean ?? null));
</script>

<div class="stack-lg">
	<PageHeader
		title="Compare runs"
		description="Runs side by side: only cells that ran the same configuration share a row. What differs between the deployments is listed first."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}

	<form class="card stack" method="GET">
		<h2>Runs</h2>
		<div class="table-wrap">
			<table>
				<thead><tr><th></th><th>Run</th><th>Target</th><th>Ended</th></tr></thead>
				<tbody>
					{#each data.runs as r (r.id)}
						<tr>
							<td
								><input
									type="checkbox"
									value={r.id}
									bind:group={chosen}
									aria-label="Compare this run"
								/></td
							>
							<td>{r.label || r.id.slice(-8)}</td>
							<td>{r.target.name}</td>
							<td class="muted small">{dateTime(r.finished_at)}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<input type="hidden" name="runs" value={chosen.join(',')} />
		<div class="row">
			<button
				class="btn btn-primary"
				type="submit"
				disabled={chosen.length < 2 || chosen.length > 6}>Compare {chosen.length} runs</button
			>
		</div>
	</form>

	{#if compared.length >= 2}
		<section class="card stack">
			<h2>What the runs ran against</h2>
			<div class="table-wrap">
				<table>
					<thead
						><tr
							><th>Field</th>{#each names as name (name)}<th>{name}</th>{/each}</tr
						></thead
					>
					<tbody>
						{#each fp as row (row.field)}
							<tr class:differs={row.differs}>
								<td>{row.field} <span class="muted small">{row.source}</span></td>
								{#each row.values as v, i (i)}<td>{v}</td>{/each}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
			<p class="muted small">
				Observed: the gateway said so. Declared: the operator did. Highlighted: the runs differ.
			</p>
		</section>

		{#if common.length === 0}
			<Notice tone="info"
				>No configuration ran in two of these runs: nothing to put side by side. External targets
				line up with others only when they declare the mock behind them.</Notice
			>
		{:else}
			<section class="card stack">
				<h2>The same configurations</h2>
				<div class="charts">
					<figure>
						<figcaption>Whole call p99</figcaption>
						<TimeChart
							label="Whole call p99"
							labels={p99.labels}
							series={p99.series}
							format={ms}
							height={260}
						/>
					</figure>
					<figure>
						<figcaption>Overhead against the baseline, p50</figcaption>
						<TimeChart
							label="Overhead p50"
							labels={overhead.labels}
							series={overhead.series}
							format={ms}
							height={260}
							emptyText="No run had a baseline."
						/>
					</figure>
					<figure>
						<figcaption>Calls a second</figcaption>
						<TimeChart
							label="Calls a second"
							labels={rps.labels}
							series={rps.series}
							format={n}
							height={260}
						/>
					</figure>
				</div>
				<div class="table-wrap">
					<table>
						<thead>
							<tr
								><th>Configuration</th>{#each names as name (name)}<th>{name}</th>{/each}</tr
							>
						</thead>
						<tbody>
							{#each common as row (row.key)}
								<tr>
									<td>{row.label}</td>
									{#each row.cells as c, i (i)}
										<td class="small">
											{#if c?.steady}
												{n(c.steady.rps_mean)}/s · p50 {ms(c.steady.total.p50)} · p99 {ms(
													c.steady.total.p99
												)}<br />
												overhead {deltaMs(c.overheadP50)} · gateway p99 {ms(c.steady.gw_delay?.p99)}
												{#if c.flags.length}<br /><span class="muted"
														>{c.flags.map((f) => f.code).join(', ')}</span
													>{/if}
											{:else if c}<Badge value={c.status} />{:else}<span class="muted">–</span>{/if}
										</td>
									{/each}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</section>
		{/if}
	{/if}
</div>

<style>
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

	tr.differs td {
		background: var(--warning-soft, transparent);
	}
</style>
