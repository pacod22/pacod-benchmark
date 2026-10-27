<script lang="ts">
	import { page } from '$app/state';

	const unavailable = $derived(page.status === 502 || page.status === 503);
</script>

<svelte:head>
	<title>{page.status} · pacod-benchmark</title>
</svelte:head>

<main>
	<div class="card stack">
		<p class="eyebrow">pacod-benchmark · error {page.status}</p>
		<h1>
			{unavailable
				? "The benchmark's API does not answer"
				: page.status === 404
					? 'Page not found'
					: 'Something went wrong'}
		</h1>
		<p>{page.error?.message}</p>
		{#if unavailable}
			<p class="muted">
				The UI could not reach pacod-bench's API, or the API could not reach pacod-id. Runs that are
				going keep going. Check that bench-api is running, then reload.
			</p>
		{/if}
		<div class="row">
			<a class="btn btn-primary" href={page.url.pathname + page.url.search} data-sveltekit-reload>
				Reload
			</a>
			<a class="btn" href="/">Go to the dashboard</a>
		</div>
	</div>
</main>

<style>
	main {
		display: grid;
		place-items: center;
		min-height: 100dvh;
		padding: var(--space-6);
	}

	.card {
		max-width: 36rem;
	}
</style>
