<script lang="ts">
	import { enhance } from '$app/forms';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime } from '#lib/format.ts';
	import { axisName } from '#lib/results.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const canWrite = $derived(data.me.permissions.includes('bench.suites.manage'));
	const cells = (axes: Record<string, unknown[]>) =>
		Object.values(axes).reduce((n, v) => n * Math.max(1, v.length), 1);
</script>

<div class="stack-lg">
	<PageHeader
		title="Suites"
		description="A suite is one configuration and the axes it varies; a run makes every combination on a target, each with a baseline straight to the mock."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}

	<div class="table-wrap card">
		<table>
			<thead
				><tr><th>Suite</th><th>Axes</th><th>Combinations</th><th>Version</th><th>Changed</th></tr
				></thead
			>
			<tbody>
				{#each data.suites as s (s.id)}
					<tr>
						<td
							><a href="/suites/{s.id}">{s.name}</a><br /><span class="muted small"
								>{s.document.description}</span
							></td
						>
						<td class="small">{Object.keys(s.document.axes).map(axisName).join(', ') || '–'}</td>
						<td
							>{cells(s.document.axes)}{s.document.repetitions > 1
								? ` × ${s.document.repetitions}`
								: ''}</td
						>
						<td>{s.version}</td>
						<td class="muted small">{dateTime(s.updated_at)}</td>
					</tr>
				{:else}
					<tr><td colspan="5" class="muted">No suite yet.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	{#if canWrite}
		<div class="grid">
			<form class="card stack" method="POST" action="?/create" use:enhance>
				<h2>New suite</h2>
				<label
					>Start from
					<select name="template">
						{#each data.templates as t (t.id)}<option value={t.id}>{t.title}</option>{/each}
					</select>
				</label>
				<div class="row">
					<button class="btn btn-primary" type="submit">Create and edit</button>
				</div>
			</form>
			<form class="card stack" method="POST" action="?/create" use:enhance>
				<h2>Import</h2>
				{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
				<label
					>A suite in YAML or JSON (pacod-benchmark's examples/suites/)
					<textarea name="yaml" rows="6" class="mono" required>{form?.yaml ?? ''}</textarea>
				</label>
				<div class="row"><button class="btn" type="submit">Import</button></div>
			</form>
		</div>
	{/if}
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(20rem, 1fr));
		gap: var(--space-5);
		align-items: start;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		font-size: var(--text-sm);
	}
</style>
