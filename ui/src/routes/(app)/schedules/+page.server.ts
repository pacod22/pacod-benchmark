import { fail } from '@sveltejs/kit';
import { body, readForm } from '#lib/schedules.ts';
import type { Schedule, SuiteRow, Target } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const [schedules, suites, targets] = await Promise.all([
		locals.api.get<{ items: Schedule[] }>('/schedules'),
		locals.api.get<{ items: SuiteRow[] }>('/suites'),
		locals.api.get<{ items: Target[] }>('/targets')
	]);
	return {
		schedules: schedules.ok ? schedules.data.items : [],
		suites: suites.ok ? suites.data.items.map((s) => ({ id: s.id, name: s.name })) : [],
		gateways: targets.ok
			? targets.data.items
					.filter((t) => t.kind === 'managed')
					.map((t) => ({ id: t.id, name: t.name }))
			: [],
		problem: schedules.ok ? null : schedules.problem.message
	};
};

const id = (form: FormData) => encodeURIComponent(String(form.get('id') ?? ''));

export const actions: Actions = {
	create: async ({ locals, request }) => {
		const read = readForm(await request.formData());
		if (typeof read === 'string') return fail(422, { message: read });
		const r = await locals.api.post<Schedule>('/schedules', read);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { created: r.data.name };
	},
	toggle: async ({ locals, request }) => {
		const form = await request.formData();
		const current = await locals.api.get<Schedule>(`/schedules/${id(form)}`);
		if (!current.ok) return fail(current.status, { message: current.problem.message });
		const r = await locals.api.put(
			`/schedules/${id(form)}`,
			body(current.data, { enabled: !current.data.enabled })
		);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { toggled: true };
	},
	fire: async ({ locals, request }) => {
		const form = await request.formData();
		const r = await locals.api.post<{ run_id: string }>(`/schedules/${id(form)}/fire`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { fired: r.data.run_id };
	},
	delete: async ({ locals, request }) => {
		const form = await request.formData();
		const r = await locals.api.del(`/schedules/${id(form)}`);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		return { deleted: true };
	}
};
