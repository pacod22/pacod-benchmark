<script lang="ts">
	import { enhance } from '$app/forms';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import ScheduleFields from '#lib/components/ScheduleFields.svelte';
	import { dateTime } from '#lib/format.ts';
	import { describe } from '#lib/schedules.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const s = $derived(data.schedule);
	const can = (p: string) => data.me.permissions.includes(p as never);
	const mayChange = $derived(
		can('bench.runs.start') &&
			(s.created_by === data.me.principal.id || can('bench.targets.manage'))
	);
</script>

<div class="stack-lg">
	<PageHeader
		title={s.name}
		description="{describe(s.cadence)}, made by {s.created_by_name}"
		crumbs={[{ label: 'Schedules', href: '/schedules' }]}
	/>
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	<p class="small">
		Next: {s.enabled ? dateTime(s.next_fire_at) : 'switched off'}. Last: {s.last_outcome ||
			'never fired'}{#if s.last_run_id}
			(<a href="/runs/{s.last_run_id}">the run</a>){/if}.
	</p>
	{#if mayChange}
		<form class="card stack" method="POST" action="?/save" use:enhance>
			<ScheduleFields
				schedule={s}
				suites={data.suites}
				gateways={data.gateways}
				agents={data.me.agent_names}
				canPublish={can('bench.reports.publish')}
			/>
			<div class="row"><button class="btn btn-primary" type="submit">Save</button></div>
		</form>
	{:else}
		<p class="muted">Only its creator or an admin changes this schedule.</p>
	{/if}
</div>
