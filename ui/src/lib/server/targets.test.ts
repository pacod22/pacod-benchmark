import { describe, expect, it } from 'vitest';
import { declared, gatewayDefinition, token, values } from './targets.ts';

function form(entries: Record<string, string>): FormData {
	const f = new FormData();
	for (const [k, v] of Object.entries(entries)) f.set(k, v);
	return f;
}

describe('the target forms', () => {
	it('fill in again without the token', () => {
		const sent = form({ name: 'eu', admin_url: 'http://gw:8081', token: 'pgwa_secret' });
		expect(values(sent)).toEqual({ name: 'eu', admin_url: 'http://gw:8081' });
		expect(JSON.stringify(values(sent))).not.toContain('pgwa_secret');
	});

	it('keep the stored token when none is typed', () => {
		expect(token(form({ token: '  ' }))).toBeUndefined();
		expect(token(form({ token: ' pgwa_x ' }))).toBe('pgwa_x');
	});

	it('read a declared fingerprint only as an object', () => {
		expect(declared(form({ declared: '' }))).toEqual({});
		expect(declared(form({ declared: '{"layout": "helm"}' }))).toEqual({ layout: 'helm' });
		expect(declared(form({ declared: '[1]' }))).toBeNull();
		expect(declared(form({ declared: '{' }))).toBeNull();
	});

	it('trim a gateway’s addresses', () => {
		expect(
			gatewayDefinition(form({ admin_url: ' http://gw:8081 ', mock_url: 'http://m:9000' }))
		).toEqual({
			admin_url: 'http://gw:8081',
			mock_url: 'http://m:9000',
			data_plane: '',
			console_url: ''
		});
	});
});
