import { error, fail, redirect } from '@sveltejs/kit';
import type { Plan, RunRow, SuiteDocument, SuiteRow, Target } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const [suite, targets] = await Promise.all([
		locals.api.get<SuiteRow>(`/suites/${encodeURIComponent(params.id)}`),
		locals.api.get<{ items: Target[] }>('/targets')
	]);
	if (!suite.ok) error(suite.status, suite.problem.message);
	return { suite: suite.data, targets: targets.ok ? targets.data.items : [] };
};

function read(form: FormData): SuiteDocument | null {
	try {
		return JSON.parse(String(form.get('document') ?? ''));
	} catch {
		return null;
	}
}

export const actions: Actions = {
	save: async ({ locals, params, request }) => {
		const document = read(await request.formData());
		if (!document) return fail(422, { message: 'The suite does not read as JSON.' });
		const r = await locals.api.put<SuiteRow>(`/suites/${encodeURIComponent(params.id)}`, {
			document
		});
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { saved: r.data.version };
	},
	plan: async ({ locals, request }) => {
		const form = await request.formData();
		const document = read(form);
		if (!document) return fail(422, { message: 'The suite does not read as JSON.' });
		const r = await locals.api.post<Plan>('/plan', {
			suite: { document },
			target_id: String(form.get('target_id') ?? '')
		});
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { plan: r.data };
	},
	start: async ({ locals, params, request }) => {
		const form = await request.formData();
		const document = read(form);
		if (!document) return fail(422, { message: 'The suite does not read as JSON.' });
		// The run starts from the suite as saved: save first, so that the run names this version.
		const saved = await locals.api.put<SuiteRow>(`/suites/${encodeURIComponent(params.id)}`, {
			document
		});
		if (!saved.ok) return fail(saved.status, { message: saved.problem.message });
		const key = String(form.get('key') ?? '').trim();
		const r = await locals.api.post<RunRow>('/runs', {
			suite_id: params.id,
			target_id: String(form.get('target_id') ?? ''),
			label: String(form.get('label') ?? '').trim(),
			acknowledge_cost: form.get('acknowledge_cost') === 'on',
			...(key ? { key } : {})
		});
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		redirect(303, `/runs/${r.data.id}`);
	},
	delete: async ({ locals, params }) => {
		const r = await locals.api.del(`/suites/${encodeURIComponent(params.id)}`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		redirect(303, '/suites');
	}
};
