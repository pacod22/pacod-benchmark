import type { AgentView } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const r = await locals.api.get<{ items: AgentView[] }>('/agents');
	return { agents: r.ok ? r.data.items : [], problem: r.ok ? null : r.problem.message };
};
