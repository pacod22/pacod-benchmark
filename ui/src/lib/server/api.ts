/**
 * pacod-bench's API, called from the UI's server with the signed-in person's token. The browser
 * never talks to the API: every page loads through its `+page.server.ts`.
 */

export interface Problem {
	status: number;
	/** The API's error type: `invalid`, `forbidden`, `not_found`, `conflict`, `unavailable`. */
	type: string;
	message: string;
}

export type Result<T> =
	{ ok: true; status: number; data: T } | { ok: false; status: number; problem: Problem };

export interface Api {
	get<T>(path: string): Promise<Result<T>>;
	post<T>(path: string, body?: unknown): Promise<Result<T>>;
	put<T>(path: string, body?: unknown): Promise<Result<T>>;
	del(path: string): Promise<Result<null>>;
	/** The raw response, for event streams and downloads. */
	raw(path: string, signal?: AbortSignal): Promise<Response | null>;
}

function parseJson(text: string): unknown {
	try {
		return text ? JSON.parse(text) : null;
	} catch {
		return null;
	}
}

const UNREACHABLE: Problem = {
	status: 502,
	type: 'unreachable',
	message: "The benchmark's API does not answer."
};

export function createApi(base: string, token: string | null, fetcher: typeof fetch = fetch): Api {
	const headers = (json: boolean): Record<string, string> => ({
		...(token ? { authorization: `Bearer ${token}` } : {}),
		...(json ? { 'content-type': 'application/json' } : {})
	});
	async function call<T>(method: string, path: string, body?: unknown): Promise<Result<T>> {
		let response: Response;
		try {
			response = await fetcher(`${base}/api/v1${path}`, {
				method,
				headers: headers(body !== undefined),
				body: body === undefined ? undefined : JSON.stringify(body)
			});
		} catch {
			return { ok: false, status: 502, problem: UNREACHABLE };
		}
		const text = await response.text();
		const data = parseJson(text);
		if (!response.ok) {
			const error = (data as { error?: { type?: string; message?: string } } | null)?.error;
			return {
				ok: false,
				status: response.status,
				problem: {
					status: response.status,
					type: error?.type ?? 'error',
					message: error?.message ?? `The API answered ${response.status}.`
				}
			};
		}
		return { ok: true, status: response.status, data: data as T };
	}
	return {
		get: (path) => call('GET', path),
		post: (path, body) => call('POST', path, body ?? {}),
		put: (path, body) => call('PUT', path, body ?? {}),
		del: (path) => call('DELETE', path),
		async raw(path, signal) {
			try {
				return await fetcher(`${base}/api/v1${path}`, { headers: headers(false), signal });
			} catch {
				return null;
			}
		}
	};
}

/** Passes an event stream of the API on to the browser. */
export async function relayEvents(api: Api, path: string, signal: AbortSignal): Promise<Response> {
	const upstream = await api.raw(path, signal);
	if (!upstream) return new Response("The benchmark's API does not answer.", { status: 502 });
	if (!upstream.ok || !upstream.body) {
		await upstream.body?.cancel();
		return new Response('The API did not open the event stream.', {
			status: upstream.status === 404 ? 404 : 502
		});
	}
	const reader = upstream.body.getReader();
	const stream = new ReadableStream<Uint8Array>({
		async pull(controller) {
			try {
				const { done, value } = await reader.read();
				if (done) controller.close();
				else controller.enqueue(value);
			} catch {
				// The browser left or the API closed the stream: end it without an error.
				controller.close();
			}
		},
		cancel(reason) {
			return reader.cancel(reason);
		}
	});
	return new Response(stream, {
		headers: {
			'content-type': 'text/event-stream',
			'cache-control': 'no-cache',
			// Tells a reverse proxy in front of the UI not to buffer the stream.
			'x-accel-buffering': 'no'
		}
	});
}
