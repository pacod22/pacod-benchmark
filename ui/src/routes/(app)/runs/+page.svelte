<script lang="ts">
	import { enhance } from '$app/forms';
	import ArrowDown from '@lucide/svelte/icons/arrow-down';
	import ArrowUp from '@lucide/svelte/icons/arrow-up';
	import ArrowUpToLine from '@lucide/svelte/icons/arrow-up-to-line';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime, duration } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	let chosen: string[] = $state([]);
	const can = (p: string) => data.me.permissions.includes(p as never);
	const leftovers = $derived(data.runs.filter((r) => r.leftovers.length).length);
	const compareHref = $derived(`/compare?runs=${chosen.join(',')}`);
</script>

<div class="stack-lg">
	<PageHeader
		title="Runs"
		description="Every run of a suite on a target, newest first. Choose two to six to compare them."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	{#if form && 'cleaned' in form && form.cleaned}
		<Notice tone={form.cleaned.left ? 'warning' : 'success'}>
			Cleaned up after {form.cleaned.runs} runs{form.cleaned.left
				? `; ${form.cleaned.left} objects are still left`
				: ''}.
		</Notice>
	{/if}

	{#if data.queue.active || data.queue.queued.length}
		<section class="card stack">
			<h2>Queue</h2>
			<p class="muted small">One run goes at a time; the others start in this order.</p>
			<ol class="queue">
				{#if data.queue.active}
					<li>
						<Badge value={data.queue.active.status} />
						<a href="/runs/{data.queue.active.id}"
							>{data.queue.active.label || data.queue.active.id.slice(-8)}</a
						>
						<span class="muted small"
							>{data.queue.active.target.name}, by {data.queue.active.started_by_name}</span
						>
					</li>
				{/if}
				{#each data.queue.queued as run, i (run.id)}
					<li>
						<Badge value="queued" />
						<a href="/runs/{run.id}">{run.label || run.id.slice(-8)}</a>
						<span class="muted small"
							>{run.target.name}, by {run.started_by_name}, about {duration(
								run.estimate?.duration_s
							)}</span
						>
						{#if can('bench.runs.start')}
							<span class="moves">
								{#each [{ to: 'top', label: 'To the front', icon: ArrowUpToLine, off: i === 0 }, { to: 'up', label: 'Up', icon: ArrowUp, off: i === 0 }, { to: 'down', label: 'Down', icon: ArrowDown, off: i === data.queue.queued.length - 1 }] as m (m.to)}
									<form method="POST" action="?/move" use:enhance>
										<input type="hidden" name="id" value={run.id} />
										<input type="hidden" name="to" value={m.to} />
										<button
											class="btn btn-quiet"
											type="submit"
											disabled={m.off}
											aria-label={m.label}
											title={m.label}><m.icon size={14} /></button
										>
									</form>
								{/each}
								<form method="POST" action="?/cancel" use:enhance>
									<input type="hidden" name="id" value={run.id} />
									<button class="btn btn-quiet btn-danger" type="submit">Cancel</button>
								</form>
							</span>
						{/if}
					</li>
				{/each}
			</ol>
		</section>
	{/if}

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
		{#if can('bench.targets.manage') && leftovers}
			<form method="POST" action="?/clean" use:enhance>
				<button class="btn" type="submit"
					>Clean up what {leftovers} runs left on their gateways</button
				>
			</form>
		{/if}
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
	.queue {
		margin: 0;
		padding-left: var(--space-5);
		display: grid;
		gap: var(--space-2);
	}

	.queue li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-2);
	}

	.moves {
		display: inline-flex;
		gap: var(--space-1);
		margin-left: auto;
	}

	.disabled {
		opacity: 0.5;
		pointer-events: none;
	}
</style>
