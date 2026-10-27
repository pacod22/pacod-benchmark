<script lang="ts">
	import { untrack } from 'svelte';
	import { enhance } from '$app/forms';
	import Plus from '@lucide/svelte/icons/plus';
	import Trash from '@lucide/svelte/icons/trash-2';
	import Badge from '#lib/components/Badge.svelte';
	import Notice from '#lib/components/Notice.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { duration, n, value } from '#lib/format.ts';
	import { axisName, cellLabel, peak } from '#lib/results.ts';
	import {
		AXES,
		cleanSuite,
		formatValues,
		parseValues,
		presetOf,
		presetStages,
		type Preset
	} from '#lib/suite.ts';
	import type { Plan, SuiteDocument } from '#lib/types.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();
	const canWrite = $derived(data.me.permissions.includes('bench.suites.manage'));
	const canRun = $derived(data.me.permissions.includes('bench.runs.start'));

	// The suite as edited: the stored one at first, then the form's. A save keeps it, so the
	// stored suite is read once.
	const initial = untrack(() => data.suite.document);
	let doc: SuiteDocument = $state(structuredClone(initial));
	let axisRows = $state(
		Object.entries(initial.axes).map(([path, values]) => ({ path, text: formatValues(values) }))
	);
	let preset: Preset | 'custom' = $state(presetOf(initial.base.load.stages) ?? 'custom');
	let level = $state(Math.max(1, ...initial.base.load.stages.map((s) => s.target)));
	let seconds = $state(initial.base.load.stages.reduce((a, s) => a + s.duration_s, 0) || 60);
	let raw = $state('');
	let rawOpen = $state(false);
	let rawProblem = $state('');
	let targetId = $state(untrack(() => data.targets[0]?.id ?? ''));
	const target = $derived(data.targets.find((t) => t.id === targetId));

	$effect(() => {
		if (preset !== 'custom') doc.base.load.stages = presetStages(preset, level, seconds);
	});

	const document = $derived.by(() => {
		const axes: Record<string, unknown[]> = {};
		for (const row of axisRows) {
			const values = parseValues(
				AXES.find((a) => a.path === row.path),
				row.text
			);
			if (row.path && values.length) axes[row.path] = values;
		}
		return cleanSuite({ ...doc, axes });
	});
	const combinations = $derived(Object.values(document.axes).reduce((k, v) => k * v.length, 1));
	const plan = $derived((form && 'plan' in form ? form.plan : null) as Plan | null);
	const unused = $derived(AXES.filter((a) => !axisRows.some((r) => r.path === a.path)));

	function addAxis() {
		const next = unused[0];
		if (next) axisRows.push({ path: next.path, text: next.options ? next.options.join(', ') : '' });
	}

	function openRaw() {
		raw = JSON.stringify(document, null, 2);
		rawProblem = '';
		rawOpen = true;
	}

	function applyRaw() {
		try {
			const parsed: SuiteDocument = JSON.parse(raw);
			doc = { ...parsed, axes: {} };
			axisRows = Object.entries(parsed.axes ?? {}).map(([path, values]) => ({
				path,
				text: formatValues(values)
			}));
			preset = presetOf(parsed.base.load.stages) ?? 'custom';
			rawOpen = false;
		} catch (e) {
			rawProblem = `Not JSON: ${(e as Error).message}`;
		}
	}

	/** Form actions keep the page's own state: only the answer changes. */
	const keep =
		() =>
		async ({
			update
		}: {
			update: (o?: { reset?: boolean; invalidateAll?: boolean }) => Promise<void>;
		}) =>
			update({ reset: false, invalidateAll: false });
</script>

<div class="stack-lg">
	<PageHeader
		title={doc.name || 'Suite'}
		description="One base configuration, the axes it varies, and the load. The plan shows every cell a run would make on a target before it starts."
		crumbs={[{ label: 'Suites', href: '/suites' }]}
	/>

	{#if form?.message}<Notice tone="error">{form.message}</Notice>{/if}
	{#if form && 'saved' in form}<Notice tone="success">Saved as version {form.saved}.</Notice>{/if}

	<section class="card stack">
		<div class="fields">
			<label class="wide"
				>Name<input bind:value={doc.name} maxlength="80" disabled={!canWrite} /></label
			>
			<label class="wide"
				>Description<input bind:value={doc.description} disabled={!canWrite} /></label
			>
		</div>
	</section>

	<div class="grid">
		<section class="card stack">
			<h2>The call</h2>
			<div class="fields">
				<label
					>Interface
					<select bind:value={doc.base.request.interface} disabled={!canWrite}>
						<option value="openai">OpenAI chat</option><option value="anthropic"
							>Anthropic messages</option
						><option value="gemini">Gemini</option><option value="native">Native chat</option
						><option value="embeddings">Embeddings</option>
					</select>
				</label>
				<label
					>Mode
					<select bind:value={doc.base.request.mode} disabled={!canWrite}
						><option value="managed">managed</option><option value="proxy">proxy</option></select
					>
				</label>
				<label
					>Prompt words<input
						type="number"
						min="1"
						bind:value={doc.base.request.prompt_words}
						disabled={!canWrite}
					/></label
				>
				<label
					>Answer tokens<input
						type="number"
						min="1"
						bind:value={doc.base.request.max_tokens}
						disabled={!canWrite}
					/></label
				>
				<label
					>HTTP
					<select bind:value={doc.base.request.http} disabled={!canWrite}
						><option value="1.1">1.1</option><option value="2">2</option></select
					>
				</label>
				<label class="check"
					><input type="checkbox" bind:checked={doc.base.request.stream} disabled={!canWrite} /> Streamed</label
				>
			</div>
		</section>

		<section class="card stack">
			<h2>The mock behind the gateway</h2>
			<div class="fields">
				<label
					>Family
					<select bind:value={doc.base.mock.family} disabled={!canWrite}
						><option>openai</option><option>anthropic</option><option>gemini</option></select
					>
				</label>
				<label
					>Delay (ms)<input
						type="number"
						min="0"
						bind:value={doc.base.mock.delay_ms}
						disabled={!canWrite}
					/></label
				>
				<label
					>Chunk delay (ms)<input
						type="number"
						min="0"
						bind:value={doc.base.mock.chunk_delay_ms}
						disabled={!canWrite}
					/></label
				>
				<label
					>Chunks<input
						type="number"
						min="1"
						bind:value={doc.base.mock.chunks}
						disabled={!canWrite}
					/></label
				>
			</div>
			<p class="muted small">
				On a managed gateway the benchmark writes this into its mock providers' URLs. An external
				target declares its own mock instead.
			</p>
		</section>

		<section class="card stack">
			<h2>The gateway's settings</h2>
			<div class="fields">
				<label
					>Response cache
					<select bind:value={doc.base.workspace.cache} disabled={!canWrite}
						><option>off</option><option>shadow</option><option>enforce</option></select
					>
				</label>
				<label
					>Audit mode
					<select bind:value={doc.base.workspace.audit} disabled={!canWrite}
						><option value="on_request">on request</option><option value="off">off</option><option
							value="on">on</option
						></select
					>
				</label>
				<label
					>Inspect (proxy)
					<select bind:value={doc.base.key.inspect} disabled={!canWrite}
						><option value={undefined}>default</option><option value="full">full</option><option
							value="none">none: blind relay</option
						></select
					>
				</label>
				<label
					>Usage policy
					<select bind:value={doc.base.key.usage_policy} disabled={!canWrite}
						><option value={undefined}>default</option><option>require</option><option
							>estimate</option
						><option>ignore</option></select
					>
				</label>
			</div>
			<p class="muted small">
				A scratch workspace and key per cell, on managed gateways only. What a deployment decides
				(analytics, Valkey, the layout) is a target of its own.
			</p>
		</section>

		<section class="card stack">
			<h2>The load</h2>
			<div class="fields">
				<label
					>Loop
					<select bind:value={doc.base.load.mode} disabled={!canWrite}
						><option value="concurrency">calls at a time</option><option value="rate"
							>calls a second</option
						></select
					>
				</label>
				<label
					>Shape
					<select bind:value={preset} disabled={!canWrite}
						><option value="steady">steady</option><option value="ramp">ramp up and down</option
						><option value="steps">four steps</option><option value="custom">custom (JSON)</option
						></select
					>
				</label>
				<label
					>Peak<input
						type="number"
						min="1"
						bind:value={level}
						disabled={!canWrite || preset === 'custom'}
					/></label
				>
				<label
					>Seconds<input
						type="number"
						min="1"
						bind:value={seconds}
						disabled={!canWrite || preset === 'custom'}
					/></label
				>
				<label
					>Steady after (s)<input
						type="number"
						min="0"
						bind:value={doc.base.load.warmup_s}
						disabled={!canWrite}
					/></label
				>
				<label
					>Timeout (s)<input
						type="number"
						min="1"
						max="600"
						bind:value={doc.base.load.timeout_s}
						disabled={!canWrite}
					/></label
				>
				{#if doc.base.load.mode === 'rate'}
					<label
						>Max in flight<input
							type="number"
							min="1"
							bind:value={doc.base.load.max_in_flight}
							disabled={!canWrite}
						/></label
					>
				{/if}
				<label
					>Calls at most<input
						type="number"
						min="0"
						bind:value={doc.base.load.max_calls}
						placeholder="no limit"
						disabled={!canWrite}
					/></label
				>
			</div>
		</section>
	</div>

	<section class="card stack">
		<div class="row between">
			<h2>Axes</h2>
			<span class="muted"
				>{combinations} combination{combinations === 1 ? '' : 's'}{doc.repetitions > 1
					? `, ${doc.repetitions} times each`
					: ''}</span
			>
		</div>
		<p class="muted small">
			Each axis sets one field to each of its values; the run makes every combination. Combinations
			that cannot run (proxy mode with another dialect than the mock's, the cache with proxy mode)
			are left out, with the reason, in the plan.
		</p>
		<div class="table-wrap">
			<table>
				<thead><tr><th>Field</th><th>Values, separated by commas</th><th></th></tr></thead>
				<tbody>
					{#each axisRows as row, i (i)}
						{@const axis = AXES.find((a) => a.path === row.path)}
						<tr>
							<td>
								<select bind:value={row.path} disabled={!canWrite} aria-label="Field">
									{#each AXES as a (a.path)}<option
											value={a.path}
											disabled={a.path !== row.path && axisRows.some((r) => r.path === a.path)}
											>{a.label} ({a.path})</option
										>{/each}
								</select>
								<br /><span class="muted small">{axis?.hint}</span>
							</td>
							<td
								><input
									bind:value={row.text}
									disabled={!canWrite}
									placeholder={axis?.options?.join(', ') ?? '10, 50, 100'}
									aria-label="Values"
								/></td
							>
							<td
								>{#if canWrite}<button
										type="button"
										class="btn btn-quiet"
										aria-label="Remove the axis"
										onclick={() => axisRows.splice(i, 1)}><Trash size={15} /></button
									>{/if}</td
							>
						</tr>
					{:else}
						<tr
							><td colspan="3" class="muted">No axis: one cell with the base configuration.</td></tr
						>
					{/each}
				</tbody>
			</table>
		</div>
		{#if canWrite}
			<div class="row">
				<button type="button" class="btn" onclick={addAxis} disabled={unused.length === 0}
					><Plus size={15} /> Add an axis</button
				>
				<label class="inline"
					>Repetitions<input type="number" min="1" max="20" bind:value={doc.repetitions} /></label
				>
				<label class="inline"
					>Order
					<select
						value={doc.order.kind}
						onchange={(e) =>
							(doc.order =
								(e.currentTarget as HTMLSelectElement).value === 'shuffled'
									? { kind: 'shuffled', seed: 1 }
									: { kind: 'grouped' })}
					>
						<option value="grouped">grouped (fewest configuration writes)</option><option
							value="shuffled">shuffled</option
						>
					</select>
				</label>
				<label class="inline check"
					><input type="checkbox" bind:checked={doc.baseline} /> Baselines straight to the mock</label
				>
				<button type="button" class="btn btn-quiet" onclick={openRaw}>Edit as JSON</button>
			</div>
		{/if}
		{#if rawOpen}
			<div class="stack">
				<textarea bind:value={raw} rows="18" class="mono"></textarea>
				{#if rawProblem}<Notice tone="error">{rawProblem}</Notice>{/if}
				<div class="row">
					<button type="button" class="btn" onclick={applyRaw}>Apply</button><button
						type="button"
						class="btn btn-quiet"
						onclick={() => (rawOpen = false)}>Close</button
					>
				</div>
			</div>
		{/if}
	</section>

	<section class="card stack">
		<h2>Plan and run</h2>
		<div class="fields">
			<label
				>Target
				<select bind:value={targetId}>
					{#each data.targets as t (t.id)}<option value={t.id}>{t.name} ({t.kind})</option>{/each}
				</select>
			</label>
		</div>
		<div class="row">
			{#if canWrite}
				<form method="POST" action="?/save" use:enhance={keep}>
					<input type="hidden" name="document" value={JSON.stringify(document)} />
					<button class="btn" type="submit">Save</button>
				</form>
			{/if}
			<form method="POST" action="?/plan" use:enhance={keep}>
				<input type="hidden" name="document" value={JSON.stringify(document)} />
				<input type="hidden" name="target_id" value={targetId} />
				<button class="btn" type="submit" disabled={!targetId}>Show the plan</button>
			</form>
		</div>

		{#if plan}
			<div class="stack">
				<dl class="estimate">
					<div>
						<dt>Cells</dt>
						<dd>{plan.estimate.cells}</dd>
					</div>
					<div>
						<dt>Baselines</dt>
						<dd>{plan.estimate.baseline_cells}</dd>
					</div>
					<div>
						<dt>About</dt>
						<dd>{duration(plan.estimate.duration_s)}</dd>
					</div>
					<div>
						<dt>Calls</dt>
						<dd>{n(plan.estimate.calls)}</dd>
					</div>
					<div>
						<dt>Configuration writes</dt>
						<dd>{plan.estimate.config_writes}</dd>
					</div>
				</dl>
				{#each plan.notes as note (note)}<Notice tone="info">{note}</Notice>{/each}
				<div class="table-wrap">
					<table>
						<thead><tr><th>#</th><th>Cell</th><th>Load</th><th>Seconds</th></tr></thead>
						<tbody>
							{#each plan.cells as c (c.index)}
								<tr>
									<td>{c.index + 1}</td>
									<td
										>{#if c.kind === 'baseline'}<Badge value="baseline" tone="neutral" />
										{/if}{cellLabel(c, true)}</td
									>
									<td>{peak(c)} {c.config.load.mode === 'rate' ? '/s' : 'at a time'}</td>
									<td>{c.config.load.stages.reduce((a, s) => a + s.duration_s, 0)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
				{#if plan.pruned.length}
					<details>
						<summary>{plan.pruned.length} combinations left out</summary>
						<ul>
							{#each plan.pruned as p, i (i)}
								<li>
									{Object.entries(p.axes)
										.map(([k, v]) => `${axisName(k)} ${value(v)}`)
										.join(', ')}: {p.reason}
								</li>
							{/each}
						</ul>
					</details>
				{/if}
			</div>
		{/if}

		{#if canRun}
			<form method="POST" action="?/start" use:enhance class="stack">
				<input type="hidden" name="document" value={JSON.stringify(document)} />
				<input type="hidden" name="target_id" value={targetId} />
				<div class="fields">
					<label
						>Label<input name="label" maxlength="80" placeholder="what this run is for" /></label
					>
					{#if target?.kind === 'external'}
						<label
							>Key of the target, for this run only (never stored)<input
								name="key"
								type="password"
								autocomplete="off"
								required
							/></label
						>
						{#if target.definition.paid}
							<label class="check"
								><input type="checkbox" name="acknowledge_cost" /> The calls may cost money</label
							>
						{/if}
					{/if}
				</div>
				{#if data.me.agent_names.length > 1}
					<fieldset class="agents">
						<legend class="small">Agents</legend>
						{#each data.me.agent_names as name (name)}
							<label class="check"
								><input
									type="checkbox"
									name="agents"
									value={name}
									checked={!data.me.default_agents.length || data.me.default_agents.includes(name)}
								/>
								<span class="mono">{name}</span></label
							>
						{/each}
					</fieldset>
				{/if}
				<div class="row">
					<button class="btn btn-primary" type="submit" disabled={!targetId}
						>Save and start the run</button
					>
				</div>
			</form>
		{/if}
	</section>

	{#if canWrite}
		<div class="row">
			<form method="POST" action="?/copy" use:enhance>
				<button class="btn" type="submit">Copy this suite</button>
			</form>
			<form method="POST" action="?/delete" use:enhance>
				<button class="btn btn-danger" type="submit">Delete this suite</button>
			</form>
		</div>
	{/if}
</div>

<style>
	fieldset.agents {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-4);
		border: 0;
		padding: 0;
		margin: 0;
	}

	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(22rem, 1fr));
		gap: var(--space-5);
		align-items: start;
	}

	.fields {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
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

	label.inline {
		flex-direction: row;
		align-items: center;
		gap: var(--space-2);
	}

	label.inline input[type='number'] {
		width: 4.5rem;
	}

	.between {
		justify-content: space-between;
	}

	.estimate {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-5);
		margin: 0;
	}

	.estimate dt {
		color: var(--text-muted);
		font-size: var(--text-xs);
	}

	.estimate dd {
		margin: 0;
		font-size: var(--text-lg, 1.25rem);
		font-weight: 600;
	}

	td input,
	td select {
		width: 100%;
	}
</style>
