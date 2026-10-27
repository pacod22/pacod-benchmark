<script lang="ts">
	import { enhance } from '$app/forms';
	import Badge from '#lib/components/Badge.svelte';
	import ExternalFields from '#lib/components/ExternalFields.svelte';
	import GatewayFields from '#lib/components/GatewayFields.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const canManage = $derived(data.me.permissions.includes('bench.targets.manage'));
	const v = $derived((form?.values ?? {}) as Record<string, string>);

	const internal = $derived(data.presets?.internal ?? null);
	const canKeepTokens = $derived(data.presets?.can_keep_tokens ?? false);
	// The gateway of this stack is offered until a target is it.
	let where: 'stack' | 'elsewhere' = $state('stack');
	const mode = $derived(internal && !internal.taken_by ? where : 'elsewhere');
	const gatewayValues: Record<string, string> = $derived(
		form?.gateway
			? v
			: mode === 'stack' && internal
				? {
						name: 'this stack',
						admin_url: internal.admin_url,
						mock_url: internal.mock_url,
						data_plane: internal.data_plane ?? ''
					}
				: {}
	);
	const externalValues: Record<string, string> = $derived(form && !form.gateway ? v : {});
</script>

<div class="stack-lg">
	<PageHeader
		title="Targets"
		description="Where runs go. A gateway the benchmark manages is a pacod-gw whose scene each run sets up through its management API: the one of this stack or another installation, from the configuration or added here. An external target (pacod-gw-lite, another product) is a URL and a model, with the key pasted for each run."
	/>
	{#if data.problem}<Notice tone="error">{data.problem}</Notice>{/if}
	<Notice tone="info">
		What only a deployment decides (analytics on or off, Valkey or memory, the scale layout, the
		minimal gateway, the hardware) is no axis of a suite: register each such deployment as a target
		of its own and run the same suite on each. Its fingerprint goes with every run.
	</Notice>

	<div class="table-wrap card">
		<table>
			<thead
				><tr
					><th>Target</th><th>Kind</th><th>Product</th><th>Address</th><th>Version seen</th><th
						>Checked</th
					></tr
				></thead
			>
			<tbody>
				{#each data.targets as t (t.id)}
					<tr>
						<td>
							<a href="/targets/{t.id}">{t.name}</a>
							{#if t.source === 'config'}<span class="muted small"> (configuration)</span>{/if}
						</td>
						<td>
							<Badge value={t.kind} tone={t.kind === 'managed' ? 'good' : 'neutral'} />
							{#if !t.usable}<Badge value="unusable" tone="bad" />{/if}
						</td>
						<td>{t.product.replaceAll('_', '-')}</td>
						<td class="mono small">{t.definition.admin_url ?? t.definition.base_url}</td>
						<td>{t.observed?.status?.software_version ?? '–'}</td>
						<td class="muted small">{dateTime(t.observed_at)}</td>
					</tr>
				{:else}
					<tr
						><td colspan="6" class="muted"
							>No target. Add a gateway or an external target below, or configure
							BENCH_GW_ADMIN_URL.</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>

	{#if canManage}
		<form class="card stack" method="POST" action="?/gateway" use:enhance>
			<h2>Add a gateway the benchmark manages</h2>
			{#if form?.gateway && form?.message}<Notice tone="error">{form.message}</Notice>{/if}
			{#if !canKeepTokens}
				<Notice tone="warning" title="The benchmark cannot keep a gateway's token">
					BENCH_KEK is not set: without it, a gateway is configured in the environment only
					(BENCH_GW_ADMIN_URL, BENCH_GW_TARGETS). The runbook says how to set the key.
				</Notice>
			{/if}
			<fieldset class="choice" disabled={!canKeepTokens}>
				<label class="check"
					><input
						type="radio"
						bind:group={where}
						value="stack"
						disabled={!internal || !!internal.taken_by}
					/> The gateway of this stack</label
				>
				<label class="check"
					><input type="radio" bind:group={where} value="elsewhere" /> Another installation (Helm, another
					host)</label
				>
			</fieldset>
			{#if internal?.taken_by}
				<p class="muted small">
					The gateway of this stack is a target already:
					<a href="/targets/{internal.taken_by.id}">{internal.taken_by.name}</a>.
				</p>
			{:else if mode === 'stack'}
				<p class="muted small">
					Filled in from this stack: its management API on pacod-admin-net and its load mock. Mint a
					token in its console (Platform → Access → Admin tokens) with the role platform admin.
				</p>
			{/if}
			<GatewayFields values={gatewayValues} tokenRequired disabled={!canKeepTokens} />
			<label
				>Declared fingerprint (JSON)<textarea
					name="declared"
					rows="3"
					disabled={!canKeepTokens}
					placeholder={'{"layout": "helm", "arch": "amd64", "cpus": 16, "mock_dedicated": true}'}
					>{gatewayValues.declared ?? ''}</textarea
				></label
			>
			<div class="row">
				<button class="btn btn-primary" type="submit" disabled={!canKeepTokens}
					>Check the token and add the gateway</button
				>
			</div>
		</form>

		<form class="card stack" method="POST" action="?/create" use:enhance>
			<h2>Add an external target</h2>
			<p class="muted small">
				pacod-gw-lite or another product: a URL and a model. The benchmark sets nothing up there,
				and the key is pasted for each run.
			</p>
			{#if form && !form.gateway && form.message}<Notice tone="error">{form.message}</Notice>{/if}
			<ExternalFields values={externalValues} />
			<label
				>Declared fingerprint (JSON)<textarea
					name="declared"
					rows="3"
					placeholder={'{"layout": "lite", "arch": "arm64", "cpus": 8, "notes": "M3 laptop"}'}
					>{externalValues.declared ?? ''}</textarea
				></label
			>
			<div class="row"><button class="btn btn-primary" type="submit">Add the target</button></div>
		</form>
	{/if}
</div>

<style>
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

	fieldset {
		border: 1px solid var(--border);
		border-radius: var(--radius);
		padding: var(--space-3);
	}

	fieldset.choice {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-4);
		border: 0;
		padding: 0;
	}
</style>
