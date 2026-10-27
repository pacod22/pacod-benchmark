import { fail } from '@sveltejs/kit';
import type { AgentView, RunSettings, SettingsView, Target } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const [settings, agents, targets] = await Promise.all([
		locals.api.get<SettingsView>('/settings'),
		locals.api.get<{ items: AgentView[] }>('/agents'),
		locals.api.get<{ items: Target[] }>('/targets')
	]);
	return {
		settings: settings.ok ? settings.data : null,
		agents: agents.ok ? agents.data.items.map((a) => a.name) : [],
		gateways: targets.ok
			? targets.data.items
					.filter((t) => t.kind === 'managed')
					.map((t) => ({ id: t.id, name: t.name }))
			: [],
		problem: settings.ok ? null : settings.problem.message
	};
};

const text = (form: FormData, name: string) => String(form.get(name) ?? '').trim();

/** A number typed in the form; an empty field is the environment's. */
function count(form: FormData, name: string): number | undefined | null {
	const raw = text(form, name);
	if (!raw) return undefined;
	const n = Number(raw);
	return Number.isInteger(n) && n >= 0 ? n : null;
}

export const actions: Actions = {
	save: async ({ locals, request }) => {
		const form = await request.formData();
		const overrides: Partial<RunSettings> = {};
		for (const name of ['max_cells', 'max_duration_s', 'agent_wait_s'] as const) {
			const n = count(form, name);
			if (n === null)
				return fail(422, { message: `${name.replaceAll('_', ' ')} is a whole number.` });
			if (n !== undefined) overrides[name] = n;
		}
		if (text(form, 'baseline_mock_url'))
			overrides.baseline_mock_url = text(form, 'baseline_mock_url');
		const agents = form.getAll('default_agents').map(String);
		if (agents.length) overrides.default_agents = agents;
		if (text(form, 'publish_target_id'))
			overrides.publish_target_id = text(form, 'publish_target_id');
		const r = await locals.api.put<SettingsView>('/settings', overrides);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { saved: true };
	}
};
