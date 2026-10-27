/**
 * Sign-in through pacod-id (BENCH_AUTH=oidc, the default). The UI is a confidential client of
 * pacod-id (a BFF), as pacod-gw's console is: `@pacod/id-sveltekit` serves /auth/login,
 * /auth/callback and /auth/logout, and keeps pacod-id's tokens sealed in one cookie;
 * `locals.token` is the session's access token (audience pacod-bench), refreshed shortly before
 * it expires. With BENCH_AUTH=dev nobody signs in: every request carries the API's
 * development token.
 */
import {
	BENCH_AUTH,
	BENCH_ID_INTERNAL_URL,
	BENCH_ID_ISSUER,
	BENCH_ORIGIN,
	BENCH_UI_CLIENT_ID,
	BENCH_UI_CLIENT_SECRET,
	BENCH_UI_SESSION_SECRET
} from '$app/env/private';
import { building } from '$app/env';
import type { Cookies } from '@sveltejs/kit';
import { createIdHandle, sealedCookie } from '@pacod/id-sveltekit';

/** The cookie's name; `__Host-` is put in front of it on https. */
const COOKIE = 'bench_session';

function required(name: string, value: string | undefined): string {
	if (!value)
		throw new Error(`${name} is required with BENCH_AUTH=oidc (signing in through pacod-id)`);
	return value;
}

function start(issuer: string) {
	const origin = required('BENCH_ORIGIN', BENCH_ORIGIN);
	const sessionSecret = required('BENCH_UI_SESSION_SECRET', BENCH_UI_SESSION_SECRET);
	if (sessionSecret.length < 32)
		throw new Error('BENCH_UI_SESSION_SECRET must have at least 32 characters');
	const handle = createIdHandle({
		issuer,
		internalUrl: BENCH_ID_INTERNAL_URL ?? issuer,
		clientId: BENCH_UI_CLIENT_ID,
		clientSecret: required('BENCH_UI_CLIENT_SECRET', BENCH_UI_CLIENT_SECRET),
		redirectUri: `${origin}/auth/callback`,
		postLogoutRedirectUri: `${origin}/`,
		origin,
		store: sealedCookie({ name: COOKIE, secret: sessionSecret }),
		secret: sessionSecret
	});
	const secure = origin.startsWith('https:');
	return {
		issuer,
		handle,
		/** Where a sign-in starts, coming back to `next`. */
		loginPath: (next: string) => handle.loginPath(next),
		/** Forgets the session in this browser (the API refused its token), as the store would. */
		forget(cookies: Cookies): void {
			cookies.delete(secure ? `__Host-${COOKIE}` : COOKIE, { path: '/', secure });
		}
	};
}

/**
 * Signing in through pacod-id, or `null` in development without it. The build loads this module
 * to analyse the server, without the deployment's settings: it starts nothing then.
 */
export const identity =
	BENCH_AUTH === 'oidc' && !building ? start(required('BENCH_ID_ISSUER', BENCH_ID_ISSUER)) : null;
