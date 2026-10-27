import { fail, redirect } from '@sveltejs/kit';
import * as forms from '#lib/server/targets.ts';
import type { Target, TargetPresets } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const [r, presets] = await Promise.all([
		locals.api.get<{ items: Target[] }>('/targets'),
		locals.api.get<TargetPresets>('/targets/presets')
	]);
	return {
		targets: r.ok ? r.data.items : [],
		presets: presets.ok ? presets.data : null,
		problem: r.ok ? null : r.problem.message
	};
};

export const actions: Actions = {
	/** A gateway the benchmark manages: its addresses and its platform-admin token. */
	gateway: async ({ locals, request }) => {
		const form = await request.formData();
		const declared = forms.declared(form);
		if (!declared)
			return fail(422, {
				gateway: true,
				message: 'The declared fingerprint is not a JSON object.',
				values: forms.values(form)
			});
		const r = await locals.api.post<Target>('/targets', {
			name: forms.name(form),
			kind: 'managed',
			definition: forms.gatewayDefinition(form),
			declared,
			token: forms.token(form)
		});
		if (!r.ok)
			return fail(r.status, {
				gateway: true,
				message: r.problem.message,
				values: forms.values(form)
			});
		redirect(303, `/targets/${r.data.id}`);
	},
	create: async ({ locals, request }) => {
		const form = await request.formData();
		const declared = forms.declared(form);
		if (!declared)
			return fail(422, {
				gateway: false,
				message: 'The declared fingerprint is not a JSON object.',
				values: forms.values(form)
			});
		const r = await locals.api.post<Target>('/targets', {
			name: forms.name(form),
			product: forms.product(form),
			definition: forms.externalDefinition(form),
			declared
		});
		if (!r.ok)
			return fail(r.status, {
				gateway: false,
				message: r.problem.message,
				values: forms.values(form)
			});
		redirect(303, `/targets/${r.data.id}`);
	}
};
