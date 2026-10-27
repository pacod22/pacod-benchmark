import { fail, redirect } from '@sveltejs/kit';
import type { Target } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const r = await locals.api.get<{ items: Target[] }>('/targets');
	return { targets: r.ok ? r.data.items : [], problem: r.ok ? null : r.problem.message };
};

const text = (form: FormData, name: string) => String(form.get(name) ?? '').trim();
const num = (form: FormData, name: string) => Number(text(form, name));

export const actions: Actions = {
	create: async ({ locals, request }) => {
		const form = await request.formData();
		let declared: unknown;
		try {
			declared = text(form, 'declared') ? JSON.parse(text(form, 'declared')) : {};
		} catch {
			return fail(422, {
				message: 'The declared fingerprint is not JSON.',
				values: Object.fromEntries(form)
			});
		}
		const definition: Record<string, unknown> = {
			base_url: text(form, 'base_url'),
			model: text(form, 'model'),
			provider: text(form, 'provider'),
			upstream: text(form, 'upstream'),
			paid: form.get('paid') === 'on'
		};
		if (form.get('declare_mock') === 'on') {
			definition.mock = {
				family: text(form, 'mock_family') || 'openai',
				delay_ms: num(form, 'mock_delay_ms'),
				chunk_delay_ms: num(form, 'mock_chunk_delay_ms'),
				chunks: num(form, 'mock_chunks')
			};
		}
		const r = await locals.api.post<Target>('/targets', {
			name: text(form, 'name'),
			product: text(form, 'product'),
			definition,
			declared
		});
		if (!r.ok)
			return fail(r.status, { message: r.problem.message, values: Object.fromEntries(form) });
		redirect(303, `/targets/${r.data.id}`);
	}
};
