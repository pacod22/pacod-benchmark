<script lang="ts">
	import { onMount } from 'svelte';
	import { afterNavigate } from '$app/navigation';
	import { page } from '$app/state';
	import CalendarClock from '@lucide/svelte/icons/calendar-clock';
	import Crosshair from '@lucide/svelte/icons/crosshair';
	import GitCompare from '@lucide/svelte/icons/git-compare';
	import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
	import ListChecks from '@lucide/svelte/icons/list-checks';
	import LogOut from '@lucide/svelte/icons/log-out';
	import Menu from '@lucide/svelte/icons/menu';
	import Moon from '@lucide/svelte/icons/moon';
	import Play from '@lucide/svelte/icons/play';
	import Server from '@lucide/svelte/icons/server';
	import Settings from '@lucide/svelte/icons/settings';
	import Sun from '@lucide/svelte/icons/sun';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	const NAV = [
		{ href: '/', label: 'Dashboard', icon: LayoutDashboard },
		{ href: '/runs', label: 'Runs', icon: Play },
		{ href: '/suites', label: 'Suites', icon: ListChecks },
		{ href: '/schedules', label: 'Schedules', icon: CalendarClock },
		{ href: '/compare', label: 'Compare', icon: GitCompare },
		{ href: '/targets', label: 'Targets', icon: Crosshair },
		{ href: '/agents', label: 'Agents', icon: Server },
		{ href: '/settings', label: 'Settings', icon: Settings }
	];

	const current = (href: string) =>
		href === '/' ? page.url.pathname === '/' : page.url.pathname.startsWith(href);
	const role = $derived(
		data.me.permissions.includes('bench.targets.manage')
			? 'Admin'
			: data.me.permissions.includes('bench.runs.start')
				? 'Operator'
				: 'Viewer'
	);

	let menuOpen = $state(false);
	let theme: 'light' | 'dark' = $state('light');

	afterNavigate(() => (menuOpen = false));

	onMount(() => {
		const chosen = document.documentElement.dataset.theme;
		theme =
			chosen === 'dark' || chosen === 'light'
				? chosen
				: matchMedia('(prefers-color-scheme: dark)').matches
					? 'dark'
					: 'light';
	});

	function toggleTheme() {
		theme = theme === 'dark' ? 'light' : 'dark';
		document.documentElement.dataset.theme = theme;
		try {
			localStorage.setItem('bench-theme', theme);
		} catch {
			// Storage can be blocked; the choice then lasts until the page is reloaded.
		}
	}
</script>

<a class="skip" href="#main">Skip to the content</a>

<div class="shell">
	{#if menuOpen}
		<div class="scrim" role="presentation" onclick={() => (menuOpen = false)}></div>
	{/if}
	<aside class="sidebar" class:open={menuOpen} aria-label="Navigation">
		<div class="brand">
			<a href="/" class="brand-link" aria-label="pacod-benchmark: dashboard">
				<span class="mark" aria-hidden="true">BM<span class="dot"></span></span>
				<span class="brand-text">
					<span class="brand-name">pacod-benchmark</span>
					<span class="brand-sub">Gateway benchmarks</span>
				</span>
			</a>
		</div>
		<nav aria-label="Main">
			<ul>
				{#each NAV as item (item.href)}
					{@const Icon = item.icon}
					<li>
						<a href={item.href} aria-current={current(item.href) ? 'page' : undefined}>
							<Icon size={16} aria-hidden="true" />
							<span>{item.label}</span>
						</a>
					</li>
				{/each}
			</ul>
		</nav>
		{#if data.me.active_run}
			<a class="active-run" href="/runs/{data.me.active_run}">
				<span class="pulse" aria-hidden="true"></span> A run is going
			</a>
		{/if}
	</aside>

	<div class="column">
		<header class="topbar">
			<button
				type="button"
				class="icon-button hamburger"
				aria-label="Open the navigation"
				aria-expanded={menuOpen}
				onclick={() => (menuOpen = true)}
			>
				<Menu size={20} />
			</button>
			<div class="spacer"></div>
			<button
				type="button"
				class="icon-button"
				aria-label={theme === 'dark' ? 'Switch to the light theme' : 'Switch to the dark theme'}
				onclick={toggleTheme}
			>
				{#if theme === 'dark'}<Sun size={18} />{:else}<Moon size={18} />{/if}
			</button>
			<div class="who">
				<span class="who-name" title={data.me.principal.id}>{data.me.principal.display}</span>
				<span class="who-role">{role}</span>
			</div>
			{#if data.signOut}
				<form method="POST" action={data.signOut}>
					<button type="submit" class="icon-button" aria-label="Sign out" title="Sign out">
						<LogOut size={18} />
					</button>
				</form>
			{/if}
		</header>
		<main id="main" tabindex="-1">
			{@render children()}
		</main>
	</div>
</div>

<style>
	.skip {
		position: absolute;
		left: var(--space-2);
		top: -4rem;
		z-index: 50;
		padding: var(--space-2) var(--space-3);
		border-radius: var(--radius);
		background: var(--accent);
		color: var(--on-accent);
	}

	.skip:focus {
		top: var(--space-2);
	}

	.shell {
		display: grid;
		grid-template-columns: var(--sidebar-width) minmax(0, 1fr);
		min-height: 100dvh;
	}

	.sidebar {
		position: sticky;
		top: 0;
		z-index: 30;
		align-self: start;
		display: flex;
		flex-direction: column;
		height: 100dvh;
		overflow-y: auto;
		border-right: 1px solid var(--sidebar-border);
		background: var(--sidebar);
		color: var(--sidebar-text);
	}

	.brand {
		display: flex;
		align-items: center;
		height: var(--topbar-height);
		padding: 0 var(--space-4);
		border-bottom: 1px solid var(--sidebar-border);
	}

	.brand-link {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		color: inherit;
	}

	.brand-link:hover {
		color: inherit;
		text-decoration: none;
	}

	.mark {
		position: relative;
		display: grid;
		place-items: center;
		width: 2rem;
		height: 2rem;
		border-radius: var(--radius);
		background: var(--accent);
		color: var(--on-accent);
		font-family: var(--font-display);
		font-size: var(--text-xs);
		font-weight: 800;
		letter-spacing: 0.02em;
	}

	.dot {
		position: absolute;
		right: -0.15rem;
		bottom: -0.15rem;
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--gold);
	}

	.brand-text {
		display: flex;
		flex-direction: column;
		line-height: 1.15;
	}

	.brand-name {
		font-family: var(--font-display);
		font-weight: 700;
	}

	.brand-sub {
		color: var(--sidebar-muted, var(--text-muted));
		font-size: var(--text-xs);
	}

	nav ul {
		margin: 0;
		padding: var(--space-3) var(--space-2);
		list-style: none;
	}

	nav a {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		padding: var(--space-2) var(--space-3);
		border-radius: var(--radius);
		color: inherit;
	}

	nav a:hover {
		background: var(--sidebar-hover, var(--surface-2));
		text-decoration: none;
	}

	nav a[aria-current='page'] {
		background: var(--sidebar-active, var(--accent-soft));
		color: var(--sidebar-active-text, var(--accent));
		font-weight: 600;
	}

	.active-run {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		margin: auto var(--space-3) var(--space-4);
		padding: var(--space-2) var(--space-3);
		border: 1px solid var(--sidebar-border);
		border-radius: var(--radius);
		color: inherit;
		font-size: var(--text-sm);
	}

	.pulse {
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--success);
		animation: pulse 1.6s ease-in-out infinite;
	}

	@keyframes pulse {
		50% {
			opacity: 0.3;
		}
	}

	.column {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}

	.topbar {
		position: sticky;
		top: 0;
		z-index: 20;
		display: flex;
		align-items: center;
		gap: var(--space-3);
		height: var(--topbar-height);
		padding: 0 var(--space-5);
		border-bottom: 1px solid var(--border);
		background: var(--surface);
	}

	.spacer {
		flex: 1;
	}

	.icon-button {
		display: inline-grid;
		place-items: center;
		width: 2.25rem;
		height: 2.25rem;
		border: 1px solid transparent;
		border-radius: var(--radius);
		background: none;
		color: var(--text-muted);
		cursor: pointer;
	}

	.icon-button:hover {
		border-color: var(--border);
		color: var(--text);
	}

	.hamburger {
		display: none;
	}

	.who {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		line-height: 1.2;
	}

	.who-name {
		font-weight: 600;
		font-size: var(--text-sm);
	}

	.who-role {
		color: var(--text-muted);
		font-size: var(--text-xs);
	}

	main {
		width: 100%;
		max-width: var(--content-width);
		margin: 0 auto;
		padding: var(--space-6) var(--space-5) var(--space-7);
	}

	main:focus {
		outline: none;
	}

	.scrim {
		position: fixed;
		inset: 0;
		z-index: 25;
		background: rgb(0 0 0 / 0.35);
	}

	@media (max-width: 64rem) {
		.shell {
			grid-template-columns: minmax(0, 1fr);
		}

		.sidebar {
			position: fixed;
			left: 0;
			transform: translateX(-100%);
			transition: transform 0.2s ease;
			width: var(--sidebar-width);
		}

		.sidebar.open {
			transform: none;
		}

		.hamburger {
			display: inline-grid;
		}
	}
</style>
