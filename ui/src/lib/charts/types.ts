/** One series of a chart: the shape pacod-gw's console uses (ui/src/lib/report/stack.ts there). */
export interface ChartSeries {
	name: string;
	/** `null` is a gap: nothing was measured at that point. */
	values: (number | null)[];
	/** A colour variable of the theme: `--series-1`, `--danger`. */
	colour: string;
	/** A column among lines. */
	bar?: boolean;
}
