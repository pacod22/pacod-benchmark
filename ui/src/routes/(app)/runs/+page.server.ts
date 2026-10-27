import { fail } from '@sveltejs/kit';
import type { RunRow } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const before = url.searchParams.get('before');
	const [r, queue] = await Promise.all([
		locals.api.get<{ items: RunRow[]; next_before: string | null }>(
			`/runs?limit=50${before ? `&before=${encodeURIComponent(before)}` : ''}`
		),
		locals.api.get<{ active: RunRow | null; queued: RunRow[] }>('/queue')
	]);
	return {
		runs: r.ok ? r.data.items : [],
		next: r.ok ? r.data.next_before : null,
		queue: queue.ok ? queue.data : { active: null, queued: [] },
		problem: r.ok ? null : r.problem.message
	};
};

const id = (form: FormData) => encodeURIComponent(String(form.get('id') ?? ''));

export const actions: Actions = {
	move: async ({ locals, request }) => {
		const form = await request.formData();
		const r = await locals.api.post(`/runs/${id(form)}/move`, { to: String(form.get('to') ?? '') });
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { moved: true };
	},
	cancel: async ({ locals, request }) => {
		const form = await request.formData();
		const r = await locals.api.post(`/runs/${id(form)}/cancel`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { cancelled: true };
	},
	clean: async ({ locals }) => {
		const r = await locals.api.post<{ runs: number; left: number }>('/leftovers/clean');
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { cleaned: r.data };
	}
};
