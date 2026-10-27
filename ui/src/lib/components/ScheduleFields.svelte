<!-- The fields of a schedule: what runs, where, when (UTC), with which agents, and whether its report is published. -->
<script lang="ts">
	import { WEEKDAYS } from '#lib/schedules.ts';
	import type { Schedule } from '#lib/types.ts';

	interface Props {
		/** The schedule changed; none for a new one. */
		schedule?: Schedule | null;
		suites: { id: string; name: string }[];
		/** Managed gateways only: an external target's key is never kept. */
		gateways: { id: string; name: string }[];
		agents: string[];
		canPublish: boolean;
	}

	let { schedule = null, suites, gateways, agents, canPublish }: Props = $props();
	const initial = () =>
		schedule?.cadence ?? { kind: 'daily' as const, at: '02:30', weekdays: [1, 2, 3, 4, 5] };
	let kind: 'daily' | 'every' = $state(initial().kind);
	const c = $derived(schedule?.cadence ?? initial());
</script>

<div class="fields">
	<label
		>Name<input
			name="name"
			required
			maxlength="80"
			value={schedule?.name ?? ''}
			placeholder="nightly regression"
		/></label
	>
	<label
		>Suite<select name="suite_id" required value={schedule?.suite_id ?? suites[0]?.id ?? ''}>
			{#each suites as s (s.id)}<option value={s.id}>{s.name}</option>{/each}
		</select></label
	>
	<label
		>Gateway<select name="target_id" required value={schedule?.target_id ?? gateways[0]?.id ?? ''}>
			{#each gateways as g (g.id)}<option value={g.id}>{g.name}</option>{/each}
		</select></label
	>
	<label
		>Label of its runs<input
			name="label"
			maxlength="80"
			value={schedule?.label ?? ''}
			placeholder="schedule: its name"
		/></label
	>
</div>

<fieldset class="when">
	<legend class="small">When, in UTC</legend>
	<label class="check"
		><input type="radio" name="kind" value="daily" bind:group={kind} /> Daily at</label
	>
	<input
		name="at"
		type="time"
		value={c.kind === 'daily' ? c.at : '02:30'}
		disabled={kind !== 'daily'}
		aria-label="Time, UTC"
	/>
	{#each WEEKDAYS as day, i (day)}
		<label class="check"
			><input
				type="checkbox"
				name="weekdays"
				value={i + 1}
				checked={c.kind === 'daily' ? c.weekdays.includes(i + 1) : i < 5}
				disabled={kind !== 'daily'}
			/>
			{day}</label
		>
	{/each}
	<label class="check"
		><input type="radio" name="kind" value="every" bind:group={kind} /> Every</label
	>
	<input
		name="hours"
		type="number"
		min="1"
		max="168"
		value={c.kind === 'every' ? c.hours : 6}
		disabled={kind !== 'every'}
		aria-label="Hours"
	/>
	<span class="small">hours</span>
</fieldset>
<p class="muted small">
	No day ticked: every day. A fire missed while the benchmark was stopped is not made up for.
</p>

{#if agents.length > 1}
	<fieldset class="when">
		<legend class="small">Agents (none ticked: the settings' default ones)</legend>
		{#each agents as name (name)}
			<label class="check"
				><input
					type="checkbox"
					name="agents"
					value={name}
					checked={schedule?.agents?.includes(name) ?? false}
				/>
				<span class="mono">{name}</span></label
			>
		{/each}
	</fieldset>
{/if}

<div class="row">
	<label class="check"
		><input type="checkbox" name="enabled" checked={schedule?.enabled ?? true} /> On</label
	>
	<label class="check"
		><input
			type="checkbox"
			name="publish"
			checked={schedule?.publish ?? false}
			disabled={!canPublish}
		/> Publish the report to pacod-gw when the run has finished</label
	>
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

	fieldset.when {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-3);
		border: 0;
		padding: 0;
		margin: 0;
	}

	fieldset.when input[type='number'] {
		width: 5rem;
	}
</style>
