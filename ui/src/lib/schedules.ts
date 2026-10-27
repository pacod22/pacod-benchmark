/**
 * Schedules in the UI: their cadence in words, and the schedule form read into the API's body.
 */

import type { Cadence, Schedule } from './types.ts';

export const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

/** "every weekday at 02:30 UTC", "every 6 hours". */
export function describe(c: Cadence): string {
	if (c.kind === 'every') return c.hours === 1 ? 'every hour' : `every ${c.hours} hours`;
	const days = [...c.weekdays].sort((a, b) => a - b);
	const which =
		days.length === 0 || days.length === 7
			? 'every day'
			: days.join() === '1,2,3,4,5'
				? 'every weekday'
				: days.join() === '6,7'
					? 'at weekends'
					: `on ${days.map((d) => WEEKDAYS[d - 1]).join(', ')}`;
	return `${which} at ${c.at} UTC`;
}

const text = (form: FormData, name: string) => String(form.get(name) ?? '').trim();

/** The API's body of the schedule form; a sentence when it does not read. */
export function readForm(form: FormData): Record<string, unknown> | string {
	let cadence: Cadence;
	if (text(form, 'kind') === 'every') {
		const hours = Number(text(form, 'hours'));
		if (!Number.isInteger(hours) || hours < 1) return 'The hours are a whole number, 1 or more.';
		cadence = { kind: 'every', hours };
	} else {
		const at = text(form, 'at');
		if (!/^\d{2}:\d{2}$/.test(at)) return 'The time is HH:MM.';
		cadence = { kind: 'daily', at, weekdays: form.getAll('weekdays').map(Number) };
	}
	const agents = form.getAll('agents').map(String);
	return {
		name: text(form, 'name'),
		suite_id: text(form, 'suite_id'),
		target_id: text(form, 'target_id'),
		label: text(form, 'label'),
		cadence,
		enabled: form.get('enabled') === 'on',
		publish: form.get('publish') === 'on',
		...(agents.length ? { agents } : {})
	};
}

/** A schedule as the API takes it back: to switch it on or off. */
export function body(s: Schedule, changes: Partial<Schedule> = {}): Record<string, unknown> {
	const t = { ...s, ...changes };
	return {
		name: t.name,
		suite_id: t.suite_id,
		target_id: t.target_id,
		label: t.label,
		cadence: t.cadence,
		enabled: t.enabled,
		publish: t.publish,
		...(t.agents ? { agents: t.agents } : {})
	};
}
