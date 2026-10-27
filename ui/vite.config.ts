import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { CSP_DIRECTIVES } from './src/lib/csp.ts';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Runes everywhere but in libraries.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			adapter: adapter(),
			// Nonces on the pages it renders: every page is rendered per request.
			csp: { mode: 'auto', directives: CSP_DIRECTIVES }
		})
	],
	build: {
		// Fonts and images stay files: the policy allows them from 'self', not as data: URLs.
		assetsInlineLimit: 0
	}
});
