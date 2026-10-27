import type { ComparedRun } from '#lib/compare.ts';
import type { RunRow } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const ids = (url.searchParams.get('runs') ?? '').split(',').filter(Boolean);
	const list = await locals.api.get<{ items: RunRow[] }>('/runs?limit=100');
	const runs = list.ok
		? list.data.items.filter((r) => r.status === 'finished' || r.status === 'cancelled')
		: [];
	if (ids.length < 2) return { chosen: ids, runs, compared: null, problem: null };
	const r = await locals.api.get<{ runs: ComparedRun[] }>(
		`/compare?runs=${ids.map(encodeURIComponent).join(',')}`
	);
	return {
		chosen: ids,
		runs,
		compared: r.ok ? r.data.runs : null,
		problem: r.ok ? null : r.problem.message
	};
};
