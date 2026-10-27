import { error } from '@sveltejs/kit';
import type { RequestHandler } from './$types';

/** The run's report (pacod-bench-report/1), as a file to carry to another installation. */
export const GET: RequestHandler = async ({ locals, params }) => {
	const upstream = await locals.api.raw(`/runs/${encodeURIComponent(params.id)}/report`);
	if (!upstream) error(502, "The benchmark's API does not answer.");
	if (!upstream.ok) error(upstream.status, (await upstream.text()) || 'No report for this run.');
	return new Response(upstream.body, {
		headers: {
			'content-type': 'application/json',
			'content-disposition': `attachment; filename="pacod-bench-${params.id}.json"`
		}
	});
};
