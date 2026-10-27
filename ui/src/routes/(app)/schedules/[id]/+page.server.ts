import { error, fail, redirect } from '@sveltejs/kit';
import { readForm } from '#lib/schedules.ts';
import type { Schedule, SuiteRow, Target } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const [schedule, suites, targets] = await Promise.all([
		locals.api.get<Schedule>(`/schedules/${encodeURIComponent(params.id)}`),
		locals.api.get<{ items: SuiteRow[] }>('/suites'),
		locals.api.get<{ items: Target[] }>('/targets')
	]);
	if (!schedule.ok) error(schedule.status, schedule.problem.message);
	return {
		schedule: schedule.data,
		suites: suites.ok ? suites.data.items.map((s) => ({ id: s.id, name: s.name })) : [],
		gateways: targets.ok
			? targets.data.items
					.filter((t) => t.kind === 'managed')
					.map((t) => ({ id: t.id, name: t.name }))
			: []
	};
};

export const actions: Actions = {
	save: async ({ locals, params, request }) => {
		const read = readForm(await request.formData());
		if (typeof read === 'string') return fail(422, { message: read });
		const r = await locals.api.put(`/schedules/${encodeURIComponent(params.id)}`, read);
		if (!r.ok) return fail(r.status, { message: r.problem.message });
		redirect(303, '/schedules');
	}
};
