<script lang="ts">
	import { enhance } from '$app/forms';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime, ms } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const t = $derived(data.target);
	const can = (p: string) => data.me.permissions.includes(p as never);
	const check = $derived(
		(form && 'check' in form ? form.check : null) as {
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
</script>

<div class="stack-lg">
	<PageHeader
		title={t.name}
		description={t.kind === 'managed'
			? 'A pacod-gw the benchmark manages: each run sets its scene up through the management API.'
			: 'An external target: a URL and a model. The benchmark sets nothing up there.'}
		crumbs={[{ label: 'Targets', href: '/targets' }]}
	/>
	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}

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

	{#if can('bench.targets.manage') && t.source === 'api'}
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
