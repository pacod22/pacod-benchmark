<script lang="ts">
	import { enhance } from '$app/forms';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import ScheduleFields from '#lib/components/ScheduleFields.svelte';
	import { dateTime } from '#lib/format.ts';
	import { describe } from '#lib/schedules.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const can = (p: string) => data.me.permissions.includes(p as never);
	const mine = (createdBy: string) =>
		createdBy === data.me.principal.id || can('bench.targets.manage');
	const names = (list: { id: string; name: string }[], id: string) =>
		list.find((x) => x.id === id)?.name ?? id;
</script>

<div class="stack-lg">
	<PageHeader
		title="Schedules"
		description="Runs on a timetable: a suite on a gateway the benchmark manages, daily or every few hours, in UTC. A schedule fires with its creator's authority, checked again at every fire."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	{#if form && 'fired' in form && form.fired}
		<Notice tone="success">Queued: <a href="/runs/{form.fired}">the run</a>.</Notice>
	{/if}

	<div class="table-wrap card">
		<table>
			<thead
				><tr
					><th>Schedule</th><th>When</th><th>Runs</th><th>Next</th><th>Last</th
					>{#if can('bench.runs.start')}<th></th>{/if}</tr
				></thead
			>
			<tbody>
				{#each data.schedules as s (s.id)}
					<tr>
						<td>
							<a href="/schedules/{s.id}">{s.name}</a>
							{#if !s.enabled}<Badge value="off" tone="neutral" />{/if}
							{#if s.publish}<Badge value="publishes" tone="good" />{/if}
							<br /><span class="muted small">by {s.created_by_name}</span>
						</td>
						<td>{describe(s.cadence)}</td>
						<td class="small"
							>{names(data.suites, s.suite_id)}<br />on {names(data.gateways, s.target_id)}</td
						>
						<td class="small">{s.enabled ? dateTime(s.next_fire_at) : '–'}</td>
						<td class="small">
							{#if s.last_fire_at}{dateTime(s.last_fire_at)}<br />{/if}
							{#if s.last_run_id}<a href="/runs/{s.last_run_id}">{s.last_outcome}</a
								>{:else}{s.last_outcome || '–'}{/if}
						</td>
						{#if can('bench.runs.start')}
							<td class="actions">
								<form method="POST" action="?/fire" use:enhance>
									<input type="hidden" name="id" value={s.id} />
									<button class="btn btn-quiet" type="submit">Fire now</button>
								</form>
								{#if mine(s.created_by)}
									<form method="POST" action="?/toggle" use:enhance>
										<input type="hidden" name="id" value={s.id} />
										<button class="btn btn-quiet" type="submit"
											>{s.enabled ? 'Switch off' : 'Switch on'}</button
										>
									</form>
									<form method="POST" action="?/delete" use:enhance>
										<input type="hidden" name="id" value={s.id} />
										<button class="btn btn-quiet btn-danger" type="submit">Remove</button>
									</form>
								{/if}
							</td>
						{/if}
					</tr>
				{:else}
					<tr><td colspan="6" class="muted">No schedule yet.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	{#if can('bench.runs.start')}
		<form class="card stack" method="POST" action="?/create" use:enhance>
			<h2>Add a schedule</h2>
			{#if !data.gateways.length}
				<Notice tone="info"
					>A schedule runs on a gateway the benchmark manages; there is none yet.</Notice
				>
			{/if}
			<ScheduleFields
				suites={data.suites}
				gateways={data.gateways}
				agents={data.me.agent_names}
				canPublish={can('bench.reports.publish')}
			/>
			<div class="row">
				<button
					class="btn btn-primary"
					type="submit"
					disabled={!data.gateways.length || !data.suites.length}>Add the schedule</button
				>
			</div>
		</form>
	{/if}
</div>

<style>
	.actions {
		white-space: nowrap;
	}

	.actions form {
		display: inline;
	}
</style>
