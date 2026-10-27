import type { RunRow } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
	const before = url.searchParams.get('before');
	const r = await locals.api.get<{ items: RunRow[]; next_before: string | null }>(
		`/runs?limit=50${before ? `&before=${encodeURIComponent(before)}` : ''}`
	);
	return {
		runs: r.ok ? r.data.items : [],
		next: r.ok ? r.data.next_before : null,
		problem: r.ok ? null : r.problem.message
	};
};
