<script lang="ts">
	import { enhance } from '$app/forms';
	import Badge from '#lib/components/Badge.svelte';
	import ExternalFields from '#lib/components/ExternalFields.svelte';
	import GatewayFields from '#lib/components/GatewayFields.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime, ms } from '#lib/format.ts';
	import type { TokenCheck } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const t = $derived(data.target);
	const can = (p: string) => data.me.permissions.includes(p as never);
	const check = $derived(
		(form && 'check' in form ? form.check : null) as {
			token: TokenCheck | null;
			observed: typeof t.observed;
			probe: {
				ok: boolean;
				status: number | null;
				ttfb_ms: number;
				total_ms: number;
				error: string;
				excerpt: string;
				settled: boolean | null;
				leftovers: string[];
			} | null;
		} | null
	);
	const observed = $derived(check?.observed ?? t.observed);
	const components = $derived(observed?.status?.components ?? []);
	const tokenCheck = $derived(check?.token ?? t.token_checked);
	const editable = $derived(t.source === 'api' && can('bench.targets.manage'));
	// A refused change is filled in again from what it sent (never the token), else from the target.
	const sent = $derived(
		form && 'values' in form ? ((form.values ?? {}) as Record<string, string>) : null
	);
	const gatewayValues: Record<string, string> = $derived(
		sent ?? {
			name: t.name,
			admin_url: t.definition.admin_url ?? '',
			mock_url: t.definition.mock_url ?? '',
			data_plane: t.definition.data_plane ?? '',
			console_url: t.definition.console_url ?? ''
		}
	);
	const externalValues: Record<string, string> = $derived(
		sent ?? {
			name: t.name,
			product: t.product,
			base_url: t.definition.base_url ?? '',
			model: t.definition.model ?? '',
			provider: t.definition.provider ?? '',
			upstream: t.definition.upstream ?? '',
			...(t.definition.paid ? { paid: 'on' } : {})
		}
	);
</script>

<div class="stack-lg">
	<PageHeader
		title={t.name}
		description={t.kind === 'managed'
			? t.source === 'config'
				? 'A pacod-gw the benchmark manages, from its configuration: each run sets its scene up through the management API.'
				: 'A pacod-gw the benchmark manages, added here with its token: each run sets its scene up through the management API.'
			: 'An external target: a URL and a model. The benchmark sets nothing up there.'}
		crumbs={[{ label: 'Targets', href: '/targets' }]}
	/>
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	{#if !t.usable}
		<Notice tone="warning" title="This gateway takes no run now">
			{t.unusable_reason}
		</Notice>
	{/if}

	<div class="grid">
		<section class="card stack">
			<h2>Definition</h2>
			<dl class="kv">
				<dt>Kind</dt>
				<dd><Badge value={t.kind} /> {t.source === 'config' ? '(from the configuration)' : ''}</dd>
				<dt>Product</dt>
				<dd>{t.product.replaceAll('_', '-')}</dd>
				{#if t.kind === 'managed'}
					<dt>Management API</dt>
					<dd class="mono">{t.definition.admin_url}</dd>
					<dt>Mock, as the gateway reaches it</dt>
					<dd class="mono">{t.definition.mock_url}</dd>
					<dt>Data plane, as the agents reach it</dt>
					<dd class="mono">{t.definition.data_plane ?? "each agent's own target"}</dd>
					{#if t.source === 'api'}
						<dt>Console</dt>
						<dd class="mono">{t.definition.console_url ?? 'BENCH_GW_CONSOLE_URL'}</dd>
						<dt>Token</dt>
						<dd class="mono">{t.token_hint || 'none'}</dd>
					{:else}
						<dt>Token</dt>
						<dd>BENCH_GW_ADMIN_TOKEN, or the token file of BENCH_GW_TARGETS</dd>
					{/if}
					{#if tokenCheck}
						<dt>The gateway takes it for</dt>
						<dd>
							{tokenCheck.display}
							{tokenCheck.platform_admin ? '(platform admin)' : ''}
							<span class="muted small">at {dateTime(tokenCheck.checked_at)}</span>
						</dd>
					{/if}
				{:else}
					<dt>Base URL</dt>
					<dd class="mono">{t.definition.base_url}</dd>
					<dt>Model</dt>
					<dd class="mono">{t.definition.model}</dd>
					{#if t.definition.provider}<dt>Proxy provider</dt>
						<dd class="mono">{t.definition.provider} / {t.definition.upstream}</dd>{/if}
					<dt>Paid</dt>
					<dd>
						{t.definition.paid
							? 'may reach a paid provider: every cell needs a number of calls'
							: 'no'}
					</dd>
					<dt>Mock behind it</dt>
					<dd>
						{t.definition.mock
							? `${t.definition.mock.family}, ${t.definition.mock.delay_ms} ms, ${t.definition.mock.chunks} chunks of ${t.definition.mock.chunk_delay_ms} ms`
							: 'not declared: its cells are compared with no other target’s'}
					</dd>
				{/if}
			</dl>
		</section>

		<section class="card stack">
			<h2>Observed</h2>
			{#if observed?.status}
				<dl class="kv">
					<dt>Version</dt>
					<dd>{observed.status.software_version}</dd>
					<dt>Instances</dt>
					<dd>
						{observed.status.instances?.length ?? '–'} ({observed.status.instances
							?.map((i) => i.role)
							.join(', ')})
					</dd>
					<dt>Cache store</dt>
					<dd>{observed.cache?.store ?? '–'}</dd>
					{#each components as c (c.name)}<dt>{c.name}</dt>
						<dd><Badge value={c.status} /></dd>{/each}
				</dl>
				<p class="muted small">Read {dateTime(t.observed_at)}.</p>
			{:else}
				<p class="muted">
					{t.kind === 'managed'
						? 'Not checked yet.'
						: 'An external target describes nothing of itself: its fingerprint is what you declare.'}
				</p>
			{/if}
			{#if can('bench.runs.start')}
				<form method="POST" action="?/check" use:enhance class="stack">
					{#if t.kind === 'external'}
						<label
							>Key, for this check only (never stored)<input
								name="key"
								type="password"
								autocomplete="off"
							/></label
						>
					{/if}
					<div class="row"><button class="btn" type="submit">Check now, with one call</button></div>
				</form>
			{/if}
			{#if check?.probe}
				<Notice
					tone={check.probe.ok ? 'success' : 'error'}
					title={check.probe.ok ? 'The call went through' : 'The call did not go through'}
				>
					<p>
						Status {check.probe.status ?? '–'}, first byte {ms(check.probe.ttfb_ms)}, whole call {ms(
							check.probe.total_ms
						)}{check.probe.settled === false
							? '; the instances had not applied the probe’s key in time'
							: ''}.
					</p>
					{#if check.probe.error}<p>{check.probe.error}</p>{/if}
					{#if check.probe.excerpt}<pre class="small">{check.probe.excerpt}</pre>{/if}
					{#if check.probe.leftovers.length}<p>
							Left over: {check.probe.leftovers.join(', ')}
						</p>{/if}
				</Notice>
			{/if}
		</section>
	</div>

	<section class="card stack">
		<h2>Declared fingerprint</h2>
		<p class="muted small">
			What only you know about the deployment: layout (single, scale, nodes, helm, lite), arch,
			CPUs, memory, notes. It goes with every run on this target, marked as declared. Set
			"mock_dedicated": true when no other load shares this gateway's mock.
		</p>
		{#if can('bench.targets.manage')}
			<form method="POST" action="?/declared" use:enhance class="stack">
				<textarea name="declared" rows="5" class="mono"
					>{JSON.stringify(t.declared, null, 2)}</textarea
				>
				<div class="row">
					<button class="btn" type="submit">Save</button>
					{#if form && 'saved' in form}<span class="muted small">Saved.</span>{/if}
				</div>
			</form>
		{:else}
			<pre>{JSON.stringify(t.declared, null, 2)}</pre>
		{/if}
	</section>

	{#if editable && t.kind === 'managed'}
		<form class="card stack" method="POST" action="?/gateway" use:enhance>
			<h2>Change the gateway</h2>
			<p class="muted small">
				Not while a run on it waits or goes, or left objects on it. A new management API needs the
				token again.
			</p>
			<GatewayFields values={gatewayValues} tokenRequired={false} />
			<input type="hidden" name="declared" value={JSON.stringify(t.declared)} />
			<div class="row">
				<button class="btn" type="submit">Save</button>
				{#if form && 'saved' in form}<span class="muted small">Saved.</span>{/if}
			</div>
		</form>
	{:else if editable}
		<form class="card stack" method="POST" action="?/external" use:enhance>
			<h2>Change the target</h2>
			<ExternalFields values={externalValues} mock={t.definition.mock ?? null} editing />
			<input type="hidden" name="declared" value={JSON.stringify(t.declared)} />
			<div class="row">
				<button class="btn" type="submit">Save</button>
				{#if form && 'saved' in form}<span class="muted small">Saved.</span>{/if}
			</div>
		</form>
	{/if}

	{#if editable}
		<form method="POST" action="?/delete" use:enhance>
			<button class="btn btn-danger" type="submit">Delete this target</button>
		</form>
	{/if}
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(22rem, 1fr));
		gap: var(--space-5);
		align-items: start;
	}

	.kv {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		gap: var(--space-2) var(--space-4);
		margin: 0;
	}

	.kv dt {
		color: var(--text-muted);
	}

	.kv dd {
		margin: 0;
		overflow-wrap: anywhere;
	}

	label {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		font-size: var(--text-sm);
	}

	pre {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
</style>
