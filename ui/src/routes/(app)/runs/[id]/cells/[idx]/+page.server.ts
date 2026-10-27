import { error } from '@sveltejs/kit';
import type { CellRow, RunPage } from '#lib/types.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, params }) => {
	const id = encodeURIComponent(params.id);
	const [cell, page] = await Promise.all([
		locals.api.get<CellRow>(`/runs/${id}/cells/${Number(params.idx)}`),
		locals.api.get<RunPage>(`/runs/${id}`)
	]);
	if (!cell.ok) error(cell.status, cell.problem.message);
	if (!page.ok) error(page.status, page.problem.message);
	const baseline =
		cell.data.kind === 'target'
			? (page.data.cells.find(
					(c) => c.kind === 'baseline' && c.comparison_key === cell.data.baseline_key
				) ?? null)
			: null;
	return { cell: cell.data, run: page.data.run, baseline };
};
