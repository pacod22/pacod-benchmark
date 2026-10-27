import { defineEnvVars } from '@sveltejs/kit/env';

/** An optional http(s) URL setting, without a trailing slash. */
function optionalUrl(name: string, example: string) {
	return (value: string | undefined) => {
		const text = (value ?? '').trim();
		if (text === '') return undefined;
		let url: URL;
		try {
			url = new URL(text);
		} catch {
			throw new Error(`${name} must be a URL such as ${example}, got "${text}"`);
		}
		if (url.protocol !== 'http:' && url.protocol !== 'https:') {
			throw new Error(`${name} must start with http:// or https://`);
		}
		return text.replace(/\/+$/, '');
	};
}

/** An optional secret: trimmed, empty when unset. */
const secret = (value: string | undefined) => (value ?? '').trim() || undefined;

export const variables = defineEnvVars({
	BENCH_API_URL: {
		description:
			"Origin of pacod-bench's API, without a path. Server-only: the browser never talks to the API.",
		schema: (value) =>
			optionalUrl('BENCH_API_URL', 'http://bench-api-internal:8190')(value) ??
			'http://127.0.0.1:18190'
	},
	BENCH_AUTH: {
		description:
			"Who signs people in: oidc (pacod-id, the default) or dev (no sign-in; every request carries BENCH_DEV_TOKEN, development only). The API's setting of the same name must match. Server-only.",
		schema: (value) => {
			const mode = (value ?? '').trim() || 'oidc';
			if (mode !== 'oidc' && mode !== 'dev') {
				throw new Error(`BENCH_AUTH must be oidc or dev, got "${mode}"`);
			}
			return mode;
		}
	},
	BENCH_DEV_TOKEN: {
		description: "With BENCH_AUTH=dev: the API's development token. Server-only.",
		schema: secret
	},
	BENCH_ID_ISSUER: {
		description:
			"pacod-id's public issuer for the benchmark's realm, such as https://id.example.com. Server-only.",
		schema: optionalUrl('BENCH_ID_ISSUER', 'https://id.example.com')
	},
	BENCH_ID_INTERNAL_URL: {
		description:
			"pacod-id's internal listener as the UI's server reaches it (http://pacod-id-internal:8071); the issuer when unset. Server-only.",
		schema: optionalUrl('BENCH_ID_INTERNAL_URL', 'http://pacod-id-internal:8071')
	},
	BENCH_UI_CLIENT_ID: {
		description: "The UI's client at pacod-id. Default pacod-bench-ui. Server-only.",
		schema: (value) => (value ?? '').trim() || 'pacod-bench-ui'
	},
	BENCH_UI_CLIENT_SECRET: {
		description: "The UI's client secret at pacod-id. Server-only.",
		schema: secret
	},
	BENCH_UI_SESSION_SECRET: {
		description:
			'At least 32 characters: seals the session cookie and the sign-in state. Server-only.',
		schema: secret
	},
	BENCH_ORIGIN: {
		description:
			"The UI's public origin as the browser reaches it, such as https://bench.example.com: pacod-id sends people back here. Server-only.",
		schema: optionalUrl('BENCH_ORIGIN', 'https://bench.example.com')
	}
});
