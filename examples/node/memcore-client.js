/**
 * Example memcore HTTP client (not a published SDK).
 * Uses Node 18+ global fetch. Never logs the API key.
 */

export class MemcoreApiError extends Error {
  constructor(status, payload) {
    const err = payload && typeof payload === "object" ? payload.error || {} : {};
    const code = err.code ? ` code=${err.code}` : "";
    const requestId = err.request_id ? ` request_id=${err.request_id}` : "";
    const message = err.message ? `: ${err.message}` : "";
    super(`memcore HTTP ${status}${code}${requestId}${message}`);
    this.name = "MemcoreApiError";
    this.status = status;
    this.payload = payload;
  }
}

export class MemcoreClient {
  constructor({ baseUrl, apiKey, orgId, timeoutMs = 30_000 }) {
    if (!baseUrl || !apiKey || !orgId) {
      throw new Error("baseUrl, apiKey, and orgId are required");
    }
    this.baseUrl = String(baseUrl).replace(/\/$/, "");
    this._apiKey = apiKey;
    this.orgId = orgId;
    this.timeoutMs = timeoutMs;
  }

  _headers() {
    return {
      Authorization: `Bearer ${this._apiKey}`,
      "X-Organization-ID": this.orgId,
      "Content-Type": "application/json",
      Accept: "application/json",
    };
  }

  async _request(method, path, { query, body } = {}) {
    const url = new URL(`${this.baseUrl}${path}`);
    if (query) {
      for (const [key, value] of Object.entries(query)) {
        if (value !== undefined && value !== null && value !== "") {
          url.searchParams.set(key, String(value));
        }
      }
    }

    const response = await fetch(url, {
      method,
      headers: this._headers(),
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(this.timeoutMs),
    });

    let payload;
    try {
      payload = await response.json();
    } catch {
      payload = { raw: "(non-JSON body)" };
    }

    if (!response.ok) {
      throw new MemcoreApiError(response.status, payload);
    }
    return payload;
  }

  createMemory({ userId, text }) {
    return this._request("POST", "/api/v1/memories", {
      body: {
        user_id: userId,
        messages: [{ role: "user", content: text }],
        metadata: { source: "examples/node" },
      },
    });
  }

  searchMemories({ userId, query, limit = 5 }) {
    return this._request("POST", "/api/v1/memories/search", {
      body: { user_id: userId, query, limit },
    });
  }

  buildContext({ userId, query, maxTokens = 1000 }) {
    return this._request("POST", "/api/v1/context", {
      body: { user_id: userId, query, max_tokens: maxTokens },
    });
  }

  listMemories({ userId, limit = 50, cursor }) {
    return this._request("GET", `/api/v1/users/${encodeURIComponent(userId)}/memories`, {
      query: { limit, cursor },
    });
  }

  deleteMemory({ userId, memoryId }) {
    return this._request(
      "DELETE",
      `/api/v1/users/${encodeURIComponent(userId)}/memories/${encodeURIComponent(memoryId)}`,
    );
  }

  exportUser({ userId }) {
    return this._request("GET", `/api/v1/users/${encodeURIComponent(userId)}/export`, {
      query: { include_events: "true" },
    });
  }

  importUser({ userId, payload, dryRun = false }) {
    return this._request("POST", `/api/v1/users/${encodeURIComponent(userId)}/import`, {
      body: { ...payload, dry_run: dryRun },
    });
  }
}
