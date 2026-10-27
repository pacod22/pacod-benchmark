import type { Api, Problem } from '#lib/server/api.ts';
import type { Session } from '#lib/types.ts';
import type { IdSession } from '@pacod/id-sveltekit';

declare global {
	namespace App {
		interface Locals {
			/** The benchmark's API, called with this request's token. */
			api: Api;
			/** The bearer the API takes: the session's access token. It never leaves the server. */
			token: string | null;
			/** With pacod-id: the session (its tokens), refreshed when due. */
			session?: IdSession | null;
			/** Who is signed in and what they may do, asked once per request. */
			me: Session | null;
			/** Set when the API could not be asked who the caller is. */
			apiProblem: Problem | null;
		}
	}
}

export {};
