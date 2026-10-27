import { error, fail, redirect } from '@sveltejs/kit';
import type { Target } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const r = await locals.api.get<Target>(`/targets/${encodeURIComponent(params.id)}`);
	if (!r.ok) error(r.status, r.problem.message);
	return { target: r.data };
};

export const actions: Actions = {
	check: async ({ locals, params, request }) => {
		const form = await request.formData();
		const key = String(form.get('key') ?? '').trim();
		const r = await locals.api.post<{ observed: unknown; probe: unknown }>(
			`/targets/${encodeURIComponent(params.id)}/check`,
			key ? { key } : {}
		);
		if (!r.ok) return fail(r.status, { check: null, message: r.problem.message });
		return { check: r.data, message: null };
	},
	declared: async ({ locals, params, request }) => {
		const form = await request.formData();
		let declared: unknown;
		try {
			declared = JSON.parse(String(form.get('declared') ?? '{}') || '{}');
		} catch {
			return fail(422, { message: 'The declared fingerprint is not JSON.' });
		}
		const current = await locals.api.get<Target>(`/targets/${encodeURIComponent(params.id)}`);
		if (!current.ok) return fail(current.status, { message: current.problem.message });
		const t = current.data;
		const body =
			t.source === 'config'
				? { name: t.name, product: t.product, declared }
				: { name: t.name, product: t.product, definition: t.definition, declared };
		const r = await locals.api.put(`/targets/${encodeURIComponent(params.id)}`, body);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { saved: true };
	},
	delete: async ({ locals, params }) => {
		const r = await locals.api.del(`/targets/${encodeURIComponent(params.id)}`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		redirect(303, '/targets');
	}
};
