import { localAccessToken } from '../auth/localIdentity';

type GraphQLError = {
  message: string;
  extensions?: { code?: string };
};

type GraphQLResponse<T> = {
  data?: T;
  errors?: GraphQLError[];
};

export class GraphQLRequestError extends Error {
  readonly code?: string;
  readonly status?: number;

  constructor(message: string, options: { code?: string; status?: number } = {}) {
    super(message);
    this.name = 'GraphQLRequestError';
    this.code = options.code;
    this.status = options.status;
  }
}

export function configuredGraphQLEndpoint() {
  const endpoint = import.meta.env.VITE_GRAPHQL_URL?.trim();
  return endpoint || null;
}

export function configuredGraphQLSubscriptionsEndpoint() {
  const endpoint = import.meta.env.VITE_GRAPHQL_SUBSCRIPTIONS_URL?.trim();
  return endpoint || configuredGraphQLEndpoint();
}

export async function executeGraphQL<TData, TVariables extends Record<string, unknown>>(
  endpoint: string,
  operationName: string,
  query: string,
  variables: TVariables,
  signal?: AbortSignal,
): Promise<TData> {
  const accessToken = localAccessToken();
  let response: Response;
  try {
    response = await fetch(endpoint, {
      method: 'POST',
      credentials: 'include',
      signal,
      headers: {
        'content-type': 'application/json',
        accept: 'application/graphql-response+json, application/json',
        ...(accessToken ? { authorization: `Bearer ${accessToken}` } : {}),
      },
      body: JSON.stringify({ operationName, query, variables }),
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') throw error;
    throw new GraphQLRequestError('The BFF is unreachable. Check VITE_GRAPHQL_URL and the Cosmo Router.', { code: 'NETWORK_ERROR' });
  }

  if (!response.ok) {
    throw new GraphQLRequestError(`The BFF returned HTTP ${response.status}.`, { status: response.status });
  }

  const payload = await response.json() as GraphQLResponse<TData>;
  if (payload.errors?.length) {
    throw new GraphQLRequestError(payload.errors.map((error) => error.message).join('; '), { code: payload.errors[0]?.extensions?.code });
  }
  if (!payload.data) throw new GraphQLRequestError('The BFF returned no data.', { code: 'EMPTY_RESPONSE' });
  return payload.data;
}

export async function subscribeGraphQL<TData, TVariables extends Record<string, unknown>>(
  endpoint: string,
  operationName: string,
  query: string,
  variables: TVariables,
  onData: (data: TData) => void,
  signal: AbortSignal,
): Promise<void> {
  const accessToken = localAccessToken();
  let response: Response;
  try {
    response = await fetch(endpoint, {
      method: 'POST',
      credentials: 'include',
      signal,
      headers: {
        'content-type': 'application/json',
        accept: 'text/event-stream',
        ...(accessToken ? { authorization: `Bearer ${accessToken}` } : {}),
      },
      body: JSON.stringify({ operationName, query, variables }),
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') throw error;
    throw new GraphQLRequestError('The realtime BFF is unreachable.', { code: 'NETWORK_ERROR' });
  }

  if (!response.ok) throw new GraphQLRequestError(`The realtime BFF returned HTTP ${response.status}.`, { status: response.status });
  if (!response.body) throw new GraphQLRequestError('The realtime BFF returned no event stream.', { code: 'EMPTY_RESPONSE' });

  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = '';
  while (!signal.aborted) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += value;
    const frames = buffer.split(/\r?\n\r?\n/);
    buffer = frames.pop() ?? '';
    for (const frame of frames) {
      const json = frame.split(/\r?\n/).filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trim()).join('');
      if (!json || json === '[DONE]') continue;
      const payload = JSON.parse(json) as GraphQLResponse<TData>;
      if (payload.errors?.length) throw new GraphQLRequestError(payload.errors.map((error) => error.message).join('; '), { code: payload.errors[0]?.extensions?.code });
      if (payload.data) onData(payload.data);
    }
  }
}
