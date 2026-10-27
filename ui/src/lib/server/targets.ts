/**
 * The target forms, read on the UI's server: a gateway the benchmark manages, an external
 * target, a declared fingerprint. A refused form is filled in again from what it sent, without
 * the token: a gateway's token never comes back to the browser.
 */

const text = (form: FormData, name: string) => String(form.get(name) ?? '').trim();
const num = (form: FormData, name: string) => Number(text(form, name));

/** What a form sent, to fill it in again: never the token. */
export function values(form: FormData): Record<string, string> {
	const out: Record<string, string> = {};
	for (const [key, value] of form) {
		if (key !== 'token' && typeof value === 'string') out[key] = value;
	}
	return out;
}

/** The declared fingerprint, a JSON object; null when it does not read. */
export function declared(form: FormData): Record<string, unknown> | null {
	const raw = text(form, 'declared');
	if (!raw) return {};
	try {
		const v: unknown = JSON.parse(raw);
		return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
	} catch {
		return null;
	}
}

/** A gateway's addresses; empty optional ones are left for the API to read as none. */
export function gatewayDefinition(form: FormData) {
	return {
		admin_url: text(form, 'admin_url'),
		mock_url: text(form, 'mock_url'),
		data_plane: text(form, 'data_plane'),
		console_url: text(form, 'console_url')
	};
}

/** The token, when one was typed: none keeps the stored one. */
export function token(form: FormData): string | undefined {
	return text(form, 'token') || undefined;
}

export function externalDefinition(form: FormData): Record<string, unknown> {
	const definition: Record<string, unknown> = {
		base_url: text(form, 'base_url'),
		model: text(form, 'model'),
		provider: text(form, 'provider'),
		upstream: text(form, 'upstream'),
		paid: form.get('paid') === 'on'
	};
	if (form.get('declare_mock') === 'on') {
		definition.mock = {
			family: text(form, 'mock_family') || 'openai',
			delay_ms: num(form, 'mock_delay_ms'),
			chunk_delay_ms: num(form, 'mock_chunk_delay_ms'),
			chunks: num(form, 'mock_chunks')
		};
	}
	return definition;
}

export function name(form: FormData): string {
	return text(form, 'name');
}

export function product(form: FormData): string {
	return text(form, 'product');
}
