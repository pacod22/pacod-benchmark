import { relayEvents } from '#lib/server/api.ts';
import type { RequestHandler } from './$types';

/** Passes a run's events on to the browser as the API sends them. */
export const GET: RequestHandler = async ({ locals, params, request }) =>
	relayEvents(locals.api, `/runs/${encodeURIComponent(params.id)}/events`, request.signal);
