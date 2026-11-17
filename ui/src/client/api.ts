export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export class Api {
  private token = "";
  private scope = new AbortController();
  setToken(token: string) {
    this.scope.abort();
    this.scope = new AbortController();
    this.token = token;
  }
  async request<T>(
    path: string,
    options: {
      method?: string;
      data?: unknown;
      signal?: AbortSignal;
      key?: string;
      blob?: boolean;
    } = {},
  ): Promise<T> {
    const { method = "GET", data, signal, key, blob } = options;
    const scope = this.scope;
    const headers = new Headers({ Authorization: `Bearer ${this.token}` });
    const binary = data instanceof Blob;
    if (data !== undefined)
      headers.set(
        "Content-Type",
        binary ? "application/octet-stream" : "application/json",
      );
    if (key) headers.set("Idempotency-Key", key);
    const signals = [scope.signal, AbortSignal.timeout(30_000)];
    if (signal) signals.push(signal);
    const requestSignal = AbortSignal.any(signals);
    const response = await fetch("/api" + path, {
      method,
      headers,
      body:
        data === undefined ? undefined : binary ? data : JSON.stringify(data),
      signal: requestSignal,
      cache: "no-store",
    });
    requestSignal.throwIfAborted();
    if (response.status === 401)
      window.dispatchEvent(new Event("origence:unauthorized"));
    if (!response.ok) {
      let message = `请求失败 (${response.status})`;
      try {
        const value = await response.json();
        if (typeof value.error?.message === "string")
          message = value.error.message;
      } catch {
        /* Framework errors may be plain text. */
      }
      requestSignal.throwIfAborted();
      throw new ApiError(response.status, message);
    }
    const value = await (blob ? response.blob() : response.json());
    requestSignal.throwIfAborted();
    return value as T;
  }
  get<T>(path: string, signal?: AbortSignal) {
    return this.request<T>(path, { signal });
  }
  post<T>(path: string, data: unknown, signal?: AbortSignal, key?: string) {
    return this.request<T>(path, { method: "POST", data, signal, key });
  }
}
export const api = new Api();
