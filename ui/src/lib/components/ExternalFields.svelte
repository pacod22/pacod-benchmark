<!-- The fields of an external target: a URL and a model, the mock it sits in front of. Its key is pasted for each run, never here. -->
<script lang="ts">
	import type { MockProfile } from '#lib/types.ts';

	interface Props {
		values: Record<string, string>;
		/** The mock the target declares now, when it is edited. */
		mock?: MockProfile | null;
		editing?: boolean;
	}

	let { values, mock = null, editing = false }: Props = $props();
	const initial = () => (editing ? !!mock : values.declare_mock !== undefined || !values.name);
	let declareMock = $state(initial());
	const m = $derived({
		family: values.mock_family ?? mock?.family ?? 'openai',
		delay_ms: values.mock_delay_ms ?? String(mock?.delay_ms ?? 200),
		chunk_delay_ms: values.mock_chunk_delay_ms ?? String(mock?.chunk_delay_ms ?? 20),
		chunks: values.mock_chunks ?? String(mock?.chunks ?? 20)
	});
</script>

<div class="fields">
	<label
		>Name<input
			name="name"
			required
			maxlength="60"
			value={values.name ?? ''}
			placeholder="lite on the laptop"
		/></label
	>
	<label
		>Product
		<select name="product" value={values.product ?? 'pacod_gw_lite'}>
			<option value="pacod_gw_lite">pacod-gw-lite</option>
			<option value="pacod_gw">pacod-gw, unmanaged</option>
			<option value="third_party">another product</option>
		</select>
	</label>
	<label class="wide"
		>Base URL, as the agents reach it<input
			name="base_url"
			required
			value={values.base_url ?? ''}
			placeholder="http://host.docker.internal:18280"
		/></label
	>
	<label
		>Model, as that gateway names it<input
			name="model"
			required
			value={values.model ?? ''}
			placeholder="openai/gpt-mock"
		/></label
	>
	<label
		>Proxy: provider<input
			name="provider"
			value={values.provider ?? ''}
			placeholder="for /proxy/&lt;provider&gt;/…"
		/></label
	>
	<label>Proxy: model id there<input name="upstream" value={values.upstream ?? ''} /></label>
	<label class="check"
		><input type="checkbox" name="paid" checked={values.paid === 'on'} /> Calls may reach a paid provider</label
	>
</div>
<fieldset class="stack">
	<legend
		><label class="check"
			><input type="checkbox" name="declare_mock" bind:checked={declareMock} /> The target sits in front
			of pacod-gw's mock</label
		></legend
	>
	<p class="muted small">
		Point that gateway's upstream at the mock with this profile in its URL
		(…/x-mock/delay=200,chunk_delay=20,chunks=20/v1): its cells then line up with other targets'
		behind the same mock. Without it they are compared with no other target's.
	</p>
	{#if declareMock}
		<div class="fields">
			<label
				>Family<select name="mock_family" value={m.family}
					><option>openai</option><option>anthropic</option><option>gemini</option></select
				></label
			>
			<label
				>Delay (ms)<input name="mock_delay_ms" type="number" min="0" value={m.delay_ms} /></label
			>
			<label
				>Chunk delay (ms)<input
					name="mock_chunk_delay_ms"
					type="number"
					min="0"
					value={m.chunk_delay_ms}
				/></label
			>
			<label>Chunks<input name="mock_chunks" type="number" min="1" value={m.chunks} /></label>
		</div>
	{/if}
</fieldset>

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
