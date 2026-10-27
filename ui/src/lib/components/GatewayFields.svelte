<!-- The fields of a gateway the benchmark manages: its addresses and its platform-admin token. The token field is write-only: it is never filled in, and an empty one keeps the stored token. -->
<script lang="ts">
	interface Props {
		values: Record<string, string>;
		/** A new gateway needs its token; a stored one is replaced only when one is typed. */
		tokenRequired: boolean;
		disabled?: boolean;
	}

	let { values, tokenRequired, disabled = false }: Props = $props();
</script>

<div class="fields">
	<label
		>Name<input
			name="name"
			required
			maxlength="60"
			value={values.name ?? ''}
			placeholder="eu-helm"
			{disabled}
		/></label
	>
	<label class="wide"
		>Management API, as the benchmark reaches it<input
			name="admin_url"
			required
			value={values.admin_url ?? ''}
			placeholder="https://gw-admin.eu.example.com"
			{disabled}
		/></label
	>
	<label class="wide"
		>The mock, as that gateway reaches it (in its egress allow-list)<input
			name="mock_url"
			required
			value={values.mock_url ?? ''}
			placeholder="http://pacod-gw-load-mock:9000"
			{disabled}
		/></label
	>
	<label
		>Data plane, as the agents reach it<input
			name="data_plane"
			value={values.data_plane ?? ''}
			placeholder="each agent's own target"
			{disabled}
		/></label
	>
	<label
		>Its console, for links to published reports<input
			name="console_url"
			value={values.console_url ?? ''}
			placeholder="https://gw.eu.example.com"
			{disabled}
		/></label
	>
	<label class="wide"
		>{tokenRequired
			? 'Platform-admin token (pgwa_… or pidt_…)'
			: 'A new platform-admin token (leave empty to keep the stored one)'}<input
			name="token"
			type="password"
			autocomplete="off"
			spellcheck="false"
			required={tokenRequired}
			{disabled}
		/></label
	>
</div>
<p class="muted small">
	The benchmark asks the gateway who the token is before it keeps it: it must hold the role platform
	admin at the platform. It is sealed under the benchmark's key and bound to this management API, so
	a new address needs the token again. Nobody reads it back, not even an admin.
</p>

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
</style>
