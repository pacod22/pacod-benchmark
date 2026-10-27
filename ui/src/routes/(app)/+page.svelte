<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime, duration } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const me = $derived(data.me);
	const active = $derived(data.runs.find((r) => r.id === me.active_run));
	const can = (p: string) => me.permissions.includes(p as never);
</script>

<div class="stack-lg">
	<PageHeader
		title="Benchmarks"
		description="Load the gateway in many configurations, compare the results, and publish them to pacod-gw."
	/>

	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}

	{#if active}
		<section class="card stack">
			<p class="eyebrow">Going now</p>
			<h2><a href="/runs/{active.id}">{active.label || active.suite_id || active.id}</a></h2>
			<p class="muted">
				On {active.target.name}, started by {active.started_by_name} · <Badge
					value={active.status}
				/>
			</p>
		</section>
	{/if}

	<div class="grid">
		<section class="card stack">
			<div class="row between">
				<h2>Recent runs</h2>
				<a href="/runs">All runs</a>
			</div>
			{#if data.runs.length === 0}
				<p class="muted">
					No run yet. {#if can('bench.suites.manage')}<a href="/suites">Write a suite</a> and start it.{/if}
				</p>
			{:else}
				<div class="table-wrap">
					<table>
						<thead
							><tr><th>Run</th><th>Target</th><th>Cells</th><th>Status</th><th>Started</th></tr
							></thead
						>
						<tbody>
							{#each data.runs as run (run.id)}
								<tr>
									<td><a href="/runs/{run.id}">{run.label || run.id.slice(-8)}</a></td>
									<td>{run.target.name}</td>
									<td>{run.estimate?.cells ?? '–'}</td>
									<td><Badge value={run.status} /></td>
									<td class="muted">{dateTime(run.created_at)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</section>

		<section class="card stack">
			<div class="row between">
				<h2>Agents</h2>
				<a href="/agents">Details</a>
			</div>
			{#if data.agents.length === 0}
				<p class="muted">No load agent is configured (BENCH_AGENTS).</p>
			{:else}
				<ul class="agents">
					{#each data.agents as a (a.name)}
						<li>
							<span class="mono">{a.name}</span>
							{#if !a.reachable}<Badge value="unreachable" tone="bad" />{:else if a.busy}<Badge
									value="busy"
									tone="warn"
								/>{:else}<Badge value="free" tone="good" />{/if}
							<span class="muted small"
								>{a.placement.replace('_', ' ')}{a.cpus ? `, ${a.cpus} CPUs` : ''}</span
							>
						</li>
					{/each}
				</ul>
			{/if}
			<p class="muted small">
				Limits: {me.limits.max_cells} cells, {duration(me.limits.max_duration_s)} a run.
			</p>
		</section>
	</div>
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: minmax(0, 2fr) minmax(0, 1fr);
		gap: var(--space-5);
		align-items: start;
	}

	.between {
		justify-content: space-between;
	}

	.agents {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.agents li {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}

	@media (max-width: 64rem) {
		.grid {
			grid-template-columns: minmax(0, 1fr);
		}
	}
</style>
