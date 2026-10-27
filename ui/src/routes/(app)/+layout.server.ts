import { error } from '@sveltejs/kit';
import { identity } from '#lib/server/identity.ts';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ locals }) => {
	if (locals.apiProblem)
		error(
			locals.apiProblem.status >= 500 ? 502 : locals.apiProblem.status,
			locals.apiProblem.message
		);
	if (!locals.me) error(401, 'Sign in first.');
	return { me: locals.me, signOut: identity ? '/auth/logout' : null };
};
