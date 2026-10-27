<script lang="ts">
	import { enhance } from '$app/forms';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { duration } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const canManage = $derived(data.me.permissions.includes('bench.targets.manage'));
	const s = $derived(data.settings);
</script>

<div class="stack-lg">
	<PageHeader
		title="Settings"
		description="What every run started from now uses. The environment sets the ceilings (BENCH_MAX_CELLS, BENCH_MAX_DURATION, BENCH_AGENT_WAIT_S) and what is left empty here; a run keeps the settings it was started with."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	{#if form && 'saved' in form}<Notice tone="success">Saved: runs started from now use them.</Notice
		>{/if}

	{#if s}
		<form
			class="card stack"
			method="POST"
			action="?/save"
			use:enhance={() =>
				async ({ update }) =>
					update({ reset: false })}
		>
			<fieldset class="stack" disabled={!canManage}>
				<h2>Limits</h2>
				<div class="fields">
					<label
						>Cells of a run, at most<input
							name="max_cells"
							type="number"
							min="1"
							max={s.environment.max_cells}
							value={s.overrides.max_cells ?? ''}
							placeholder={String(s.environment.max_cells)}
						/>
						<span class="muted small">At most {s.environment.max_cells} (BENCH_MAX_CELLS)</span
						></label
					>
					<label
						>Seconds of a run, at most<input
							name="max_duration_s"
							type="number"
							min="60"
							max={s.environment.max_duration_s}
							value={s.overrides.max_duration_s ?? ''}
							placeholder={String(s.environment.max_duration_s)}
						/>
						<span class="muted small"
							>At most {duration(s.environment.max_duration_s)} (BENCH_MAX_DURATION)</span
						></label
					>
					<label
						>Seconds a cell waits for a busy agent<input
							name="agent_wait_s"
							type="number"
							min="0"
							max={s.environment.agent_wait_s}
							value={s.overrides.agent_wait_s ?? ''}
							placeholder={String(s.environment.agent_wait_s)}
						/>
						<span class="muted small"
							>At most {s.environment.agent_wait_s} (BENCH_AGENT_WAIT_S); then the cell is skipped</span
						></label
					>
				</div>

				<h2>Baselines</h2>
				<label
					>The mock, as the agents reach it<input
						name="baseline_mock_url"
						value={s.overrides.baseline_mock_url ?? ''}
						placeholder={s.environment.baseline_mock_url ?? 'none: the baseline cells are skipped'}
					/></label
				>

				<h2>Agents a run uses when it names none</h2>
				{#if data.agents.length}
					<div class="checks">
						{#each data.agents as name (name)}
							<label class="check"
								><input
									type="checkbox"
									name="default_agents"
									value={name}
									checked={s.effective.default_agents.includes(name)}
								/>
								<span class="mono">{name}</span></label
							>
						{/each}
					</div>
					<p class="muted small">None ticked: every agent.</p>
				{:else}
					<p class="muted">No agent is configured.</p>
				{/if}

				<h2>Where reports are published</h2>
				<label
					>Gateway<select name="publish_target_id" value={s.overrides.publish_target_id ?? ''}>
						<option value=""
							>the configuration's (BENCH_PUBLISH_URL, else its gateway, else the only one)</option
						>
						{#each data.gateways as g (g.id)}<option value={g.id}>{g.name}</option>{/each}
					</select></label
				>
			</fieldset>
			{#if canManage}
				<div class="row"><button class="btn btn-primary" type="submit">Save</button></div>
			{:else}
				<p class="muted small">An admin changes these.</p>
			{/if}
		</form>
	{/if}
</div>

<style>
	.fields {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: var(--space-3);
	}

	label {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		font-size: var(--text-sm);
	}

	label.check {
		flex-direction: row;
		align-items: center;
		gap: var(--space-2);
	}

	.checks {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-4);
	}

	fieldset {
		border: 0;
		padding: 0;
		margin: 0;
	}

	h2 {
		margin-top: var(--space-3);
	}
</style>
