import { redirect } from '@sveltejs/kit';
import { sequence, type Handle } from '@sveltejs/kit/hooks';
import { BENCH_API_URL, BENCH_AUTH, BENCH_DEV_TOKEN } from '$app/env/private';
import { allowFormTarget } from '#lib/csp.ts';
import { createApi } from '#lib/server/api.ts';
import { identity } from '#lib/server/identity.ts';
import type { Session } from '#lib/types.ts';

/** Pages that are shown without a session. */
const PUBLIC = new Set(['/no-access']);

const issuerOrigin = identity ? new URL(identity.issuer).origin : null;

const app: Handle = async ({ event, resolve }) => {
	const { locals, cookies, url } = event;
	locals.token = identity
		? (locals.session?.accessToken ?? null)
		: BENCH_AUTH === 'dev'
			? (BENCH_DEV_TOKEN ?? null)
			: null;
	locals.api = createApi(BENCH_API_URL, locals.token);
	locals.me = null;
	locals.apiProblem = null;

	if (locals.token) {
		// Who is calling, and what they may do: asked once per request, here.
		const r = await locals.api.get<Session>('/session');
		if (r.ok) {
			locals.me = r.data;
		} else if (r.status === 401) {
			// The session has ended or was revoked: forget it.
			identity?.forget(cookies);
			locals.token = null;
		} else if (r.status === 403 && r.problem.type === 'no_access') {
			if (url.pathname !== '/no-access') redirect(303, '/no-access');
		} else {
			// The API is down or failing; the layout explains.
			locals.apiProblem = r.problem;
		}
	}

	if (!locals.me && !locals.apiProblem && !PUBLIC.has(url.pathname)) {
		if (url.pathname.endsWith('/events')) return new Response('Sign in first.', { status: 401 });
		if (identity) redirect(303, identity.loginPath(url.pathname + url.search));
		return new Response('BENCH_AUTH=dev needs BENCH_DEV_TOKEN, the same as the API takes.', {
			status: 500
		});
	}

	const response = await resolve(event);
	// With pacod-id, signing out ends at the issuer, which the policy must then allow as a form's
	// destination.
	if (issuerOrigin) {
		const policy = allowFormTarget(response.headers.get('content-security-policy'), issuerOrigin);
		if (policy) response.headers.set('content-security-policy', policy);
	}
	if (!response.headers.has('cache-control')) response.headers.set('cache-control', 'no-store');
	response.headers.set('x-content-type-options', 'nosniff');
	response.headers.set('x-frame-options', 'DENY');
	response.headers.set('referrer-policy', 'same-origin');
	return response;
};

/** With pacod-id, its sign-in routes and the session come first (`lib/server/identity.ts`). */
export const handle: Handle = identity ? sequence(identity.handle as unknown as Handle, app) : app;
