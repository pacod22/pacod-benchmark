import { fail, redirect } from '@sveltejs/kit';
import { cleanSuite, TEMPLATES } from '#lib/suite.ts';
import type { SuiteRow } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const r = await locals.api.get<{ items: SuiteRow[] }>('/suites');
	return {
		suites: r.ok ? r.data.items : [],
		templates: TEMPLATES.map((t) => ({ id: t.id, title: t.title })),
		problem: r.ok ? null : r.problem.message
	};
};

export const actions: Actions = {
	/** A new suite from a template, or from YAML that was pasted. */
	create: async ({ locals, request }) => {
		const form = await request.formData();
		const yaml = String(form.get('yaml') ?? '').trim();
		const template = TEMPLATES.find((t) => t.id === form.get('template')) ?? TEMPLATES[0];
		const r = await locals.api.post<SuiteRow>(
			'/suites',
			yaml ? { yaml } : { document: cleanSuite(template.make()) }
		);
		if (!r.ok) return fail(r.status, { message: r.problem.message, yaml });
		redirect(303, `/suites/${r.data.id}`);
	}
};
