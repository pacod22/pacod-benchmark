import { describe as group, expect, it } from 'vitest';
import { body, describe, readForm } from './schedules.ts';
import type { Schedule } from './types.ts';

function form(entries: [string, string][]): FormData {
	const f = new FormData();
	for (const [k, v] of entries) f.append(k, v);
	return f;
}

group('schedules', () => {
	it('say their cadence in words', () => {
		expect(describe({ kind: 'every', hours: 1 })).toBe('every hour');
		expect(describe({ kind: 'every', hours: 6 })).toBe('every 6 hours');
		expect(describe({ kind: 'daily', at: '02:30', weekdays: [5, 1, 2, 3, 4] })).toBe(
			'every weekday at 02:30 UTC'
		);
		expect(describe({ kind: 'daily', at: '07:00', weekdays: [] })).toBe('every day at 07:00 UTC');
		expect(describe({ kind: 'daily', at: '07:00', weekdays: [7, 6] })).toBe(
			'at weekends at 07:00 UTC'
		);
		expect(describe({ kind: 'daily', at: '07:00', weekdays: [1, 3] })).toBe(
			'on Mon, Wed at 07:00 UTC'
		);
	});

	it('read their form', () => {
		const daily = readForm(
			form([
				['name', ' nightly '],
				['suite_id', 'ste_1'],
				['target_id', 'cfg-gateway'],
				['kind', 'daily'],
				['at', '02:30'],
				['weekdays', '1'],
				['weekdays', '2'],
				['enabled', 'on']
			])
		);
		expect(daily).toEqual({
			name: 'nightly',
			suite_id: 'ste_1',
			target_id: 'cfg-gateway',
			label: '',
			cadence: { kind: 'daily', at: '02:30', weekdays: [1, 2] },
			enabled: true,
			publish: false
		});
		expect(
			readForm(
				form([
					['kind', 'every'],
					['hours', '0']
				])
			)
		).toMatch(/whole number/);
		expect(
			readForm(
				form([
					['kind', 'daily'],
					['at', '2:30']
				])
			)
		).toMatch(/HH:MM/);
	});

	it('go back to the API whole, with a change', () => {
		const s = {
			name: 'n',
			suite_id: 's',
			target_id: 't',
			label: '',
			cadence: { kind: 'every', hours: 6 },
			enabled: true,
			publish: false,
			agents: null
		} as unknown as Schedule;
		expect(body(s, { enabled: false })).toEqual({
			name: 'n',
			suite_id: 's',
			target_id: 't',
			label: '',
			cadence: { kind: 'every', hours: 6 },
			enabled: false,
			publish: false
		});
	});
});
