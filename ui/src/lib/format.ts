/** How figures are written on every page. */

/** Milliseconds, with the precision their size deserves; `n/a` for a figure that is absent. */
export function ms(value: number | null | undefined): string {
	if (value === null || value === undefined || !Number.isFinite(value)) return 'n/a';
	if (value >= 1000) return `${(value / 1000).toFixed(2)} s`;
	if (value >= 100) return `${value.toFixed(0)} ms`;
	if (value >= 10) return `${value.toFixed(1)} ms`;
	return `${value.toFixed(2)} ms`;
}

/** A difference of milliseconds, with its sign. */
export function deltaMs(value: number | null | undefined): string {
	if (value === null || value === undefined || !Number.isFinite(value)) return 'n/a';
	return `${value >= 0 ? '+' : '−'}${ms(Math.abs(value))}`;
}

export function n(value: number | null | undefined): string {
	if (value === null || value === undefined || !Number.isFinite(value)) return '–';
	return value.toLocaleString('en-GB', { maximumFractionDigits: 1 });
}

export function pct(part: number, whole: number): string {
	if (!whole) return '–';
	return `${((100 * part) / whole).toFixed(part === whole ? 0 : 1)}%`;
}

/** Seconds as `1 h 5 min`, `3 min 20 s`, `45 s`. */
export function duration(seconds: number | null | undefined): string {
	if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return '–';
	const s = Math.round(seconds);
	if (s < 60) return `${s} s`;
	if (s < 3600) return `${Math.floor(s / 60)} min${s % 60 ? ` ${s % 60} s` : ''}`;
	const m = Math.round((s % 3600) / 60);
	return `${Math.floor(s / 3600)} h${m ? ` ${m} min` : ''}`;
}

export function dateTime(iso: string | null | undefined): string {
	if (!iso) return '–';
	const d = new Date(iso);
	return d.toLocaleString('en-GB', { dateStyle: 'medium', timeStyle: 'short' });
}

/** Seconds since a cell's start as `m:ss`. */
export function clock(t: number): string {
	return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
}

/** An axis value as a reader writes it: strings without quotes. */
export function value(v: unknown): string {
	if (typeof v === 'string') return v;
	return JSON.stringify(v);
}
