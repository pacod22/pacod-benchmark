import { error, fail, redirect } from '@sveltejs/kit';
import type { CellRow, Measurement, RunPage, RunRow } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params, depends }) => {
	depends('app:run');
	const r = await locals.api.get<RunPage & { measurement: Measurement }>(
		`/runs/${encodeURIComponent(params.id)}`
	);
	if (!r.ok) error(r.status, r.problem.message);
	// The cell that runs now, with its seconds so far: the live charts start from them.
	const going = r.data.cells.find((c) => ['preparing', 'running', 'draining'].includes(c.status));
	let live: CellRow | null = null;
	if (going) {
		const c = await locals.api.get<CellRow>(
			`/runs/${encodeURIComponent(params.id)}/cells/${going.idx}`
		);
		if (c.ok) live = c.data;
	}
	return { page: r.data, live };
};

export const actions: Actions = {
	cancel: async ({ locals, params }) => {
		const r = await locals.api.post(`/runs/${encodeURIComponent(params.id)}/cancel`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { cancelling: true };
	},
	publish: async ({ locals, params }) => {
		const r = await locals.api.post<{ gw_report_id: string; link: string | null }>(
			`/runs/${encodeURIComponent(params.id)}/publish`
		);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { published: r.data };
	},
	rerun: async ({ locals, params, request }) => {
		const form = await request.formData();
		const key = String(form.get('key') ?? '').trim();
		const r = await locals.api.post<RunRow>(`/runs/${encodeURIComponent(params.id)}/rerun`, {
			acknowledge_cost: form.get('acknowledge_cost') === 'on',
			...(key ? { key } : {})
		});
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		redirect(303, `/runs/${r.data.id}`);
	},
	clean: async ({ locals }) => {
		const r = await locals.api.post<{ runs: number; left: number }>('/leftovers/clean');
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { cleaned: r.data };
	},
	delete: async ({ locals, params }) => {
		const r = await locals.api.del(`/runs/${encodeURIComponent(params.id)}`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		redirect(303, '/runs');
	}
};
