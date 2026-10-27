<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime, duration } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let chosen: string[] = $state([]);
	const compareHref = $derived(`/compare?runs=${chosen.join(',')}`);
</script>

<div class="stack-lg">
	<PageHeader
		title="Runs"
		description="Every run of a suite on a target, newest first. Choose two to six to compare them."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	<div class="row">
		<a class="btn btn-primary" href="/suites">Start a run from a suite</a>
		<a
			class="btn"
			class:disabled={chosen.length < 2 || chosen.length > 6}
			href={chosen.length >= 2 && chosen.length <= 6 ? compareHref : undefined}
			aria-disabled={chosen.length < 2}
		>
			Compare {chosen.length || ''} runs
		</a>
	</div>
	<div class="table-wrap card">
		<table>
			<thead>
				<tr
					><th><span class="visually-hidden">Compare</span></th><th>Run</th><th>Target</th><th
						>Cells</th
					><th>About</th><th>Status</th><th>By</th><th>Started</th></tr
				>
			</thead>
			<tbody>
				{#each data.runs as run (run.id)}
					<tr>
						<td
							><input
								type="checkbox"
								aria-label="Compare this run"
								value={run.id}
								bind:group={chosen}
							/></td
						>
						<td><a href="/runs/{run.id}">{run.label || run.id.slice(-8)}</a></td>
						<td
							>{run.target.name}<br /><span class="muted small"
								>{run.target.product.replaceAll('_', '-')}</span
							></td
						>
						<td
							>{run.estimate?.cells ?? '–'}{run.estimate?.baseline_cells
								? ` + ${run.estimate.baseline_cells}`
								: ''}</td
						>
						<td>{duration(run.estimate?.duration_s)}</td>
						<td
							><Badge value={run.status} />{#if run.leftovers.length}<br /><span class="small"
									>{run.leftovers.length} left over</span
								>{/if}</td
						>
						<td class="small">{run.started_by_name}</td>
						<td class="muted small">{dateTime(run.created_at)}</td>
					</tr>
				{:else}
					<tr><td colspan="8" class="muted">No run yet.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
	{#if data.next}<a href="/runs?before={encodeURIComponent(data.next)}">Older runs</a>{/if}
</div>

<style>
	.disabled {
		opacity: 0.5;
		pointer-events: none;
	}
</style>
