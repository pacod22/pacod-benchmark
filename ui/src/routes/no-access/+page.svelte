<svelte:head>
	<title>No access · pacod-benchmark</title>
</svelte:head>

<main>
	<div class="card stack">
		<p class="eyebrow">pacod-benchmark</p>
		<h1>No benchmark role</h1>
		<p>
			You are signed in, but this account holds no benchmark role. An administrator gives one in
			pacod-id: viewer to read results, operator to run suites, admin for everything.
		</p>
		<form method="POST" action="/auth/logout">
			<button type="submit" class="btn">Sign out</button>
		</form>
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
