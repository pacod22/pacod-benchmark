<script lang="ts">
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const holder = (runId: string) =>
		runId.startsWith('b') ? 'this benchmark' : "pacod-gw's tester";
</script>

<div class="stack-lg">
	<PageHeader
		title="Load agents"
		description="pacod-gw's load agents, which make the calls of every cell. They are configured with BENCH_AGENTS; the tester's load tests from the gateway's console use the same agents, and an agent runs one test at a time."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	<Notice tone="info">
		An agent in the gateway's stack shares the gateway's machine: its figures show behaviour, not
		capacity. For capacity, run agents on other machines (pacod-gw's deploy/load-agent/) and declare
		them remote in BENCH_AGENT_PLACEMENT.
	</Notice>
	<div class="table-wrap card">
		<table>
			<thead>
				<tr
					><th>Agent</th><th>State</th><th>Where</th><th>Calls to</th><th>CPUs</th><th>Caps</th><th
						>Version</th
					></tr
				>
			</thead>
			<tbody>
				{#each data.agents as a (a.name)}
					<tr>
						<td><span class="mono">{a.name}</span><br /><span class="muted small">{a.url}</span></td
						>
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
					</tr>
				{:else}
					<tr><td colspan="7" class="muted">No agent is configured.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
