/**
 * What pacod-bench's API answers, as the UI reads it. Written by hand after the API's Rust types
 * (crates/pacod-bench-engine/src/{model,collect,controller,plan}.rs and
 * crates/pacod-bench/src/store.rs); a change there changes this file.
 */

export type Permission =
	| 'bench.results.read'
	| 'bench.suites.manage'
	| 'bench.runs.start'
	| 'bench.reports.publish'
	| 'bench.targets.manage'
	| 'bench.members.manage';

export interface Session {
	principal: { id: string; display: string; kind: string };
	permissions: Permission[];
	gateways: string[];
	agents: number;
	agent_names: string[];
	/** The agents a run uses when it names none; empty: every agent. */
	default_agents: string[];
	baseline: boolean;
	publish: boolean;
	console_url: string | null;
	limits: { max_cells: number; max_duration_s: number; agent_wait_s: number };
	active_run: string | null;
}

export type Placement = 'in_stack' | 'same_host' | 'remote' | 'unknown';

export interface AgentView {
	name: string;
	url: string;
	placement: Placement;
	/** From the configuration (BENCH_AGENTS) or added through the UI. */
	source: 'config' | 'api';
	reachable: boolean;
	error?: string;
	contract?: number;
	version?: string;
	target?: string;
	cpus?: number;
	caps?: {
		max_concurrency: number;
		max_rate: number;
		max_duration_s: number;
		max_body_bytes: number;
	};
	busy?: { run_id: string; state: string } | null;
}

/** The run settings: what an admin set, over the environment's (`GET /settings`). */
export interface RunSettings {
	max_cells: number;
	max_duration_s: number;
	agent_wait_s: number;
	baseline_mock_url: string | null;
	/** Empty: every agent. */
	default_agents: string[];
	publish_target_id: string | null;
}

export interface SettingsView {
	effective: RunSettings;
	overrides: Partial<RunSettings>;
	/** The environment's settings: the ceilings of the limits. */
	environment: RunSettings;
}

export interface MockProfile {
	family: 'openai' | 'anthropic' | 'gemini';
	delay_ms: number;
	chunk_delay_ms: number;
	chunks: number;
}

export interface Target {
	id: string;
	name: string;
	kind: 'managed' | 'external';
	product: 'pacod_gw' | 'pacod_gw_lite' | 'third_party';
	source: 'config' | 'api';
	definition: {
		admin_url?: string;
		mock_url?: string;
		data_plane?: string | null;
		console_url?: string | null;
		base_url?: string;
		model?: string;
		provider?: string;
		upstream?: string;
		paid?: boolean;
		mock?: MockProfile | null;
	};
	declared: Record<string, unknown>;
	observed: Observed | null;
	observed_at: string | null;
	/** A gateway added through the UI: a label of its admin token (never the token). */
	token_hint: string;
	/** Who the gateway took the token for at its last check. */
	token_checked: TokenCheck | null;
	/** Whether a managed gateway takes runs now; when not, why. */
	usable: boolean;
	unusable_reason?: string | null;
	created_by: string;
	created_at: string;
	updated_at: string;
}

export interface TokenCheck {
	display: string;
	kind: string;
	id: string;
	platform_admin: boolean;
	checked_at: string;
}

/** What the form for a new gateway starts from (`GET /targets/presets`). */
export interface TargetPresets {
	/** The gateway of this stack, and the target that already is it. */
	internal: {
		admin_url: string;
		mock_url: string;
		data_plane: string | null;
		taken_by: { id: string; name: string } | null;
	};
	/** BENCH_KEK is set: a gateway's token can be kept. */
	can_keep_tokens: boolean;
}

/** What a managed gateway says about itself. */
export interface Observed {
	status?: {
		software_version?: string;
		config_head?: number;
		instances?: { role?: string; software_version?: string; hostname?: string }[];
		components?: { name: string; status: string; detail: string }[];
	};
	cache?: { store?: string; install?: { installed?: boolean; enabled?: boolean } };
}

export interface Stage {
	duration_s: number;
	target: number;
}

export interface CellConfig {
	request: {
		interface: 'openai' | 'anthropic' | 'gemini' | 'native' | 'embeddings';
		mode: 'managed' | 'proxy';
		stream: boolean;
		prompt_words: number;
		max_tokens: number;
		http: '1.1' | '2';
		headers?: Record<string, string>;
	};
	mock: MockProfile;
	workspace: {
		cache: 'off' | 'shadow' | 'enforce';
		audit: 'off' | 'on' | 'on_request';
		responses_store?: 'off' | 'on';
		on_spool_full?: 'keep_serving' | 'fail_closed';
	};
	key: {
		inspect?: 'full' | 'none';
		usage_policy?: 'require' | 'estimate' | 'ignore';
		max_concurrency?: number;
	};
	load: {
		mode: 'concurrency' | 'rate';
		stages: Stage[];
		max_in_flight?: number;
		timeout_s: number;
		max_calls?: number;
		warmup_s: number;
	};
}

export interface SuiteDocument {
	name: string;
	description: string;
	base: CellConfig;
	axes: Record<string, unknown[]>;
	order: { kind: 'grouped' } | { kind: 'shuffled'; seed: number };
	repetitions: number;
	cap: number;
	baseline: boolean;
}

export interface SuiteRow {
	id: string;
	name: string;
	version: number;
	document: SuiteDocument;
	created_by: string;
	created_at: string;
	updated_at: string;
}

export interface Estimate {
	cells: number;
	baseline_cells: number;
	duration_s: number;
	calls: number;
	config_writes: number;
}

export interface PlannedCell {
	index: number;
	rep: number;
	kind: 'target' | 'baseline';
	axes: Record<string, unknown>;
	config: CellConfig;
	comparison_key: string;
	baseline_key: string;
}

export interface Plan {
	cells: PlannedCell[];
	pruned: { axes: Record<string, unknown>; reason: string }[];
	estimate: Estimate;
	notes: string[];
}

/** Milliseconds. */
export interface Pct {
	p50: number;
	p95: number;
	p99: number;
	max: number;
}

export interface Summary {
	duration_s: number;
	started: number;
	calls: number;
	ok: number;
	failed: number;
	cancelled: number;
	skipped: number;
	status: Record<string, number>;
	errors: { connect: number; timeout: number; body: number; other: number; cancelled: number };
	bytes: number;
	rps_mean: number;
	rps_peak: number;
	ttfb: Pct;
	total: Pct;
	/** Absent where no answer carried `x-gw-delay-ms`: never 0. */
	gw_delay: Pct | null;
}

export interface Point {
	t: number;
	target: number;
	started: number;
	completed: number;
	ok: number;
	failed: number;
	cancelled: number;
	status: Record<string, number>;
	bytes: number;
	in_flight: number;
	skipped: number;
	ttfb: Pct | null;
	total: Pct | null;
	gw_delay: Pct | null;
	cpu_max: number | null;
	lag_max_ms: number;
	gw_in_flight: number | null;
	agents: number;
}

export interface AgentPart {
	name: string;
	placement: Placement;
	version: string;
	target: string;
	cpus: number;
	state: string;
	error: string;
	calls: number;
	cpu_max: number;
	lag_max_ms: number;
	probe?: { status: number | null; ttfb_ms: number; total_ms: number; error: string | null } | null;
}

export interface Flag {
	code: string;
	message: string;
}

export type CellStatus =
	'pending' | 'preparing' | 'running' | 'draining' | 'done' | 'skipped' | 'error' | 'cancelled';

export interface CellResult {
	index: number;
	rep: number;
	kind: 'target' | 'baseline';
	status: CellStatus;
	error: string;
	axes: Record<string, unknown>;
	config: CellConfig;
	comparison_key: string;
	baseline_key: string;
	started_at: string | null;
	finished_at: string | null;
	key_hint: string;
	agents: AgentPart[];
	summary: Summary | null;
	steady: Summary | null;
	flags: Flag[];
	noise: {
		quiet_before: boolean | null;
		excess_in_flight: number | null;
		config_head_before: number | null;
		config_head_after: number | null;
	};
	points?: Point[];
	gateway?: { t: number; in_flight: number; agents_in_flight: number }[];
}

export interface CellRow {
	run_id: string;
	idx: number;
	rep: number;
	kind: 'target' | 'baseline';
	status: CellStatus;
	axes: Record<string, unknown>;
	config: CellConfig;
	comparison_key: string;
	baseline_key: string;
	result: CellResult | null;
	points?: Point[];
	started_at: string | null;
	finished_at: string | null;
}

export type RunStatus =
	| 'queued'
	| 'preparing'
	| 'running'
	| 'cleaning'
	| 'finished'
	| 'cancelled'
	| 'error'
	| 'interrupted';

export interface RunRow {
	id: string;
	status: RunStatus;
	label: string;
	suite_id: string | null;
	suite?: SuiteDocument;
	target_id: string | null;
	target: Pick<Target, 'id' | 'name' | 'kind' | 'product' | 'definition' | 'declared'>;
	agents: { names: string[]; agents: { name: string; url: string; placement: Placement }[] };
	plan?: { estimate: Estimate; pruned: Plan['pruned']; notes: string[] };
	estimate?: Estimate;
	started_by: string;
	started_by_name: string;
	key_hint: string;
	observed: Observed | null;
	leftovers: string[];
	warnings: string[];
	error: string;
	created_at: string;
	started_at: string | null;
	finished_at: string | null;
}

export interface Publication {
	id: string;
	destination: string;
	status: 'published' | 'failed';
	gw_report_id: string;
	digest: string;
	error: string;
	published_by: string;
	published_at: string;
}

/** Whether a run's figures show the target's capacity, or only how it behaves, and why. */
export interface Measurement {
	class: 'behaviour' | 'capacity';
	reasons: string[];
}

export interface RunPage {
	run: RunRow;
	cells: CellRow[];
	publications: Publication[];
	measurement?: Measurement;
}

/** When a schedule fires, in UTC (crates/pacod-bench/src/schedules.rs). */
export type Cadence =
	{ kind: 'daily'; at: string; weekdays: number[] } | { kind: 'every'; hours: number };

export interface Schedule {
	id: string;
	name: string;
	suite_id: string;
	target_id: string;
	/** None: the settings' default agents, else every one. */
	agents: string[] | null;
	label: string;
	cadence: Cadence;
	enabled: boolean;
	publish: boolean;
	created_by: string;
	created_by_name: string;
	next_fire_at: string | null;
	last_fire_at: string | null;
	last_run_id: string | null;
	last_outcome: string;
	created_at: string;
	updated_at: string;
}
