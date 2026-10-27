<script lang="ts">
	import { enhance } from '$app/forms';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { dateTime } from '#lib/format.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const canManage = $derived(data.me.permissions.includes('bench.targets.manage'));
	const v = $derived((form?.values ?? {}) as Record<string, string>);
	let declareMock = $state(true);
</script>

<div class="stack-lg">
	<PageHeader
		title="Targets"
		description="Where runs go. The gateways the benchmark manages come from its configuration; external targets (pacod-gw-lite, another installation, another product) are a URL and a model, with the key pasted for each run."
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
						<td><a href="/targets/{t.id}">{t.name}</a></td>
						<td><Badge value={t.kind} tone={t.kind === 'managed' ? 'good' : 'neutral'} /></td>
						<td>{t.product.replaceAll('_', '-')}</td>
						<td class="mono small">{t.definition.admin_url ?? t.definition.base_url}</td>
						<td>{t.observed?.status?.software_version ?? '–'}</td>
						<td class="muted small">{dateTime(t.observed_at)}</td>
					</tr>
				{:else}
					<tr
						><td colspan="6" class="muted"
							>No target. Configure BENCH_GW_ADMIN_URL, or add an external target below.</td
						></tr
					>
				{/each}
			</tbody>
		</table>
	</div>

	{#if canManage}
		<form class="card stack" method="POST" action="?/create" use:enhance>
			<h2>Add an external target</h2>
			{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
			<div class="fields">
				<label
					>Name<input
						name="name"
						required
						maxlength="60"
						value={v.name ?? ''}
						placeholder="lite on the laptop"
					/></label
				>
				<label
					>Product
					<select name="product">
						<option value="pacod_gw_lite">pacod-gw-lite</option>
						<option value="pacod_gw">pacod-gw (another installation)</option>
						<option value="third_party">another product</option>
					</select>
				</label>
				<label class="wide"
					>Base URL, as the agents reach it<input
						name="base_url"
						required
						value={v.base_url ?? ''}
						placeholder="http://host.docker.internal:18280"
					/></label
				>
				<label
					>Model, as that gateway names it<input
						name="model"
						required
						value={v.model ?? ''}
						placeholder="openai/gpt-mock"
					/></label
				>
				<label
					>Proxy: provider<input
						name="provider"
						value={v.provider ?? ''}
						placeholder="for /proxy/&lt;provider&gt;/…"
					/></label
				>
				<label>Proxy: model id there<input name="upstream" value={v.upstream ?? ''} /></label>
				<label class="check"
					><input type="checkbox" name="paid" checked={v.paid === 'on'} /> Calls may reach a paid provider</label
				>
			</div>
			<fieldset class="stack">
				<legend
					><label class="check"
						><input type="checkbox" name="declare_mock" bind:checked={declareMock} /> The target sits
						in front of pacod-gw's mock</label
					></legend
				>
				<p class="muted small">
					Point that gateway's upstream at the mock with this profile in its URL
					(…/x-mock/delay=200,chunk_delay=20,chunks=20/v1): its cells then line up with other
					targets' behind the same mock. Without it they are compared with no other target's.
				</p>
				{#if declareMock}
					<div class="fields">
						<label
							>Family<select name="mock_family"
								><option>openai</option><option>anthropic</option><option>gemini</option></select
							></label
						>
						<label>Delay (ms)<input name="mock_delay_ms" type="number" min="0" value="200" /></label
						>
						<label
							>Chunk delay (ms)<input
								name="mock_chunk_delay_ms"
								type="number"
								min="0"
								value="20"
							/></label
						>
						<label>Chunks<input name="mock_chunks" type="number" min="1" value="20" /></label>
					</div>
				{/if}
			</fieldset>
			<label
				>Declared fingerprint (JSON)<textarea
					name="declared"
					rows="3"
					placeholder={'{"layout": "lite", "arch": "arm64", "cpus": 8, "notes": "M3 laptop"}'}
					>{v.declared ?? ''}</textarea
				></label
			>
			<div class="row"><button class="btn btn-primary" type="submit">Add the target</button></div>
		</form>
	{/if}
</div>

<style>
	.fields {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: var(--space-3);
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
</style>
