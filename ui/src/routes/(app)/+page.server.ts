import type { AgentView, RunRow } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals }) => {
	const [runs, agents] = await Promise.all([
		locals.api.get<{ items: RunRow[] }>('/runs?limit=8'),
		locals.api.get<{ items: AgentView[] }>('/agents')
	]);
	return {
		runs: runs.ok ? runs.data.items : [],
		agents: agents.ok ? agents.data.items : [],
		problem: runs.ok ? null : runs.problem.message
	};
};
