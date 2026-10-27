<script lang="ts">
	import { enhance } from '$app/forms';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import type { Placement } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const canManage = $derived(data.me.permissions.includes('bench.targets.manage'));
	const v = $derived((form && 'values' in form ? form.values : {}) as Record<string, string>);
	const holder = (runId: string) =>
		runId.startsWith('b') ? 'this benchmark' : "pacod-gw's tester";
	const PLACEMENTS: { value: Placement; label: string }[] = [
		{ value: 'remote', label: 'another machine (remote)' },
		{ value: 'same_host', label: "the gateway's machine, outside its stack" },
		{ value: 'in_stack', label: "in the gateway's stack" },
		{ value: 'unknown', label: 'not declared' }
	];
	// The agent whose row is open for a change.
	let editing: string | null = $state(null);
</script>

<div class="stack-lg">
	<PageHeader
		title="Load agents"
		description="pacod-gw's load agents, which make the calls of every cell: those of the configuration (BENCH_AGENTS) and those an admin adds here. The tester's load tests from the gateway's console may use the same agents; an agent runs one test at a time."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	<Notice tone="info">
		An agent in the gateway's stack shares the gateway's machine: its figures show behaviour, not
		capacity. For capacity, run agents on other machines (pacod-gw's deploy/load-agent/) and declare
		them remote.
	</Notice>
	<div class="table-wrap card">
		<table>
			<thead>
				<tr
					><th>Agent</th><th>State</th><th>Where</th><th>Calls to</th><th>CPUs</th><th>Caps</th><th
						>Version</th
					>{#if canManage}<th></th>{/if}</tr
				>
			</thead>
			<tbody>
				{#each data.agents as a (a.name)}
					<tr>
						<td>
							<span class="mono">{a.name}</span>
							{#if a.source === 'config'}<span class="muted small"> (configuration)</span>{/if}
							<br /><span class="muted small">{a.url}</span>
						</td>
						<td>
							{#if !a.reachable}
								<Badge value="unreachable" tone="bad" /><br /><span class="muted small"
									>{a.error}</span
								>
							{:else if a.busy}
								<Badge value="busy" tone="warn" /><br /><span class="muted small"
									>{a.busy.run_id}, {holder(a.busy.run_id)} ({a.busy.state})</span
								>
							{:else}
								<Badge value="free" tone="good" />
							{/if}
						</td>
						<td>{a.placement.replace('_', ' ')}</td>
						<td class="mono small">{a.target ?? '–'}</td>
						<td>{a.cpus ?? '–'}</td>
						<td class="small">
							{#if a.caps}{a.caps.max_concurrency} at a time, {a.caps.max_rate}/s, {a.caps
									.max_duration_s} s{:else}–{/if}
						</td>
						<td class="small">{a.version ?? '–'}{a.contract ? ` (contract ${a.contract})` : ''}</td>
						{#if canManage}
							<td class="actions">
								{#if a.source === 'api'}
									<button
										class="btn btn-quiet"
										type="button"
										onclick={() => (editing = editing === a.name ? null : a.name)}>Change</button
									>
									<form method="POST" action="?/delete" use:enhance>
										<input type="hidden" name="name" value={a.name} />
										<button class="btn btn-quiet btn-danger" type="submit">Remove</button>
									</form>
								{/if}
							</td>
						{/if}
					</tr>
					{#if editing === a.name}
						<tr>
							<td colspan="8">
								<form
									class="fields"
									method="POST"
									action="?/update"
									use:enhance={() =>
										async ({ update }) => {
											await update();
											editing = null;
										}}
								>
									<input type="hidden" name="name" value={a.name} />
									<label class="wide">URL<input name="url" required value={a.url} /></label>
									<label
										>Where it stands<select name="placement" value={a.placement}>
											{#each PLACEMENTS as p (p.value)}<option value={p.value}>{p.label}</option
												>{/each}
										</select></label
									>
									<div class="row"><button class="btn" type="submit">Save</button></div>
								</form>
							</td>
						</tr>
					{/if}
				{:else}
					<tr><td colspan="8" class="muted">No agent is configured.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	{#if canManage}
		<form class="card stack" method="POST" action="?/add" use:enhance>
			<h2>Add an agent</h2>
			{#if !data.canAdd}
				<Notice tone="warning">
					BENCH_AGENT_TOKEN is not set: every agent takes the benchmark's token, so none is added
					until the operator sets it.
				</Notice>
			{/if}
			<p class="muted small">
				The agent's URL receives the agents' token and every cell's key: add only agents on a
				private network or a VPN. The agent must take the same token (its LOAD_AGENT_TOKEN).
			</p>
			<div class="fields">
				<label
					>Name<input
						name="name"
						required
						maxlength="32"
						pattern="[a-z0-9][a-z0-9-]*"
						value={v.name ?? ''}
						placeholder="eu-1"
					/></label
				>
				<label class="wide"
					>URL<input
						name="url"
						required
						value={v.url ?? ''}
						placeholder="http://10.0.0.21:8096"
					/></label
				>
				<label
					>Where it stands<select name="placement" value={v.placement ?? 'remote'}>
						{#each PLACEMENTS as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
					</select></label
				>
			</div>
			<div class="row">
				<button class="btn btn-primary" type="submit" disabled={!data.canAdd}>Add the agent</button>
			</div>
		</form>
	{/if}
</div>

<style>
	.fields {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: var(--space-3);
		align-items: end;
	}

	.wide {
		grid-column: 1 / -1;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		font-size: var(--text-sm);
	}

	.actions {
		white-space: nowrap;
	}

	.actions form {
		display: inline;
	}
</style>
