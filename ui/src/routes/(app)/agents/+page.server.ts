import { fail } from '@sveltejs/kit';
import type { AgentView } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const r = await locals.api.get<{ items: AgentView[]; can_add: boolean }>('/agents');
	return {
		agents: r.ok ? r.data.items : [],
		canAdd: r.ok ? r.data.can_add : false,
		problem: r.ok ? null : r.problem.message
	};
};

const text = (form: FormData, name: string) => String(form.get(name) ?? '').trim();

export const actions: Actions = {
	add: async ({ locals, request }) => {
		const form = await request.formData();
		const r = await locals.api.post('/agents', {
			name: text(form, 'name'),
			url: text(form, 'url'),
			placement: text(form, 'placement')
		});
		if (!r.ok)
			return fail(r.status, { message: r.problem.message, values: Object.fromEntries(form) });
		return { done: 'added' };
	},
	update: async ({ locals, request }) => {
		const form = await request.formData();
		const name = text(form, 'name');
		const r = await locals.api.put(`/agents/${encodeURIComponent(name)}`, {
			url: text(form, 'url'),
			placement: text(form, 'placement')
		});
		if (!r.ok) return fail(r.status, { message: `${name}: ${r.problem.message}` });
		return { done: 'saved' };
	},
	delete: async ({ locals, request }) => {
		const form = await request.formData();
		const name = text(form, 'name');
		const r = await locals.api.del(`/agents/${encodeURIComponent(name)}`);
		if (!r.ok) return fail(r.status, { message: `${name}: ${r.problem.message}` });
		return { done: 'removed' };
	}
};
