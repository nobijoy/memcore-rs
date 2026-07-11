"""Example memcore HTTP client (not a published SDK)."""

from __future__ import annotations

from typing import Any, Optional

import requests


class MemcoreApiError(Exception):
    def __init__(self, status_code: int, payload: Any) -> None:
        self.status_code = status_code
        self.payload = payload
        code = None
        message = None
        request_id = None
        if isinstance(payload, dict):
            err = payload.get("error") or {}
            if isinstance(err, dict):
                code = err.get("code")
                message = err.get("message")
                request_id = err.get("request_id")
        super().__init__(
            f"memcore HTTP {status_code}"
            + (f" code={code}" if code else "")
            + (f" request_id={request_id}" if request_id else "")
            + (f": {message}" if message else "")
        )


class MemcoreClient:
    def __init__(
        self,
        base_url: str,
        api_key: str,
        org_id: str,
        *,
        timeout: float = 30.0,
        session: Optional[requests.Session] = None,
    ) -> None:
        if not base_url or not api_key or not org_id:
            raise ValueError("base_url, api_key, and org_id are required")
        self.base_url = base_url.rstrip("/")
        self._api_key = api_key
        self.org_id = org_id
        self.timeout = timeout
        self._session = session or requests.Session()

    def _headers(self) -> dict[str, str]:
        return {
            "Authorization": f"Bearer {self._api_key}",
            "X-Organization-ID": self.org_id,
            "Content-Type": "application/json",
            "Accept": "application/json",
        }

    def _request(self, method: str, path: str, **kwargs: Any) -> dict[str, Any]:
        url = f"{self.base_url}{path}"
        response = self._session.request(
            method,
            url,
            headers=self._headers(),
            timeout=self.timeout,
            **kwargs,
        )
        try:
            payload: Any = response.json()
        except ValueError:
            payload = {"raw": response.text[:500]}
        if not response.ok:
            raise MemcoreApiError(response.status_code, payload)
        if not isinstance(payload, dict):
            raise MemcoreApiError(response.status_code, {"error": {"message": "expected JSON object"}})
        return payload

    def create_memory(self, user_id: str, text: str) -> dict[str, Any]:
        return self._request(
            "POST",
            "/api/v1/memories",
            json={
                "user_id": user_id,
                "messages": [{"role": "user", "content": text}],
                "metadata": {"source": "examples/python"},
            },
        )

    def search_memories(self, user_id: str, query: str, limit: int = 5) -> dict[str, Any]:
        return self._request(
            "POST",
            "/api/v1/memories/search",
            json={"user_id": user_id, "query": query, "limit": limit},
        )

    def build_context(
        self, user_id: str, query: str, max_tokens: int = 1000
    ) -> dict[str, Any]:
        return self._request(
            "POST",
            "/api/v1/context",
            json={"user_id": user_id, "query": query, "max_tokens": max_tokens},
        )

    def list_memories(self, user_id: str, *, limit: int = 50, cursor: Optional[str] = None) -> dict[str, Any]:
        params: dict[str, Any] = {"limit": limit}
        if cursor:
            params["cursor"] = cursor
        return self._request("GET", f"/api/v1/users/{user_id}/memories", params=params)

    def delete_memory(self, user_id: str, memory_id: str) -> dict[str, Any]:
        return self._request("DELETE", f"/api/v1/users/{user_id}/memories/{memory_id}")

    def export_user(self, user_id: str) -> dict[str, Any]:
        return self._request(
            "GET",
            f"/api/v1/users/{user_id}/export",
            params={"include_events": "true"},
        )

    def import_user(
        self, user_id: str, payload: dict[str, Any], dry_run: bool = False
    ) -> dict[str, Any]:
        body = dict(payload)
        body["dry_run"] = dry_run
        return self._request("POST", f"/api/v1/users/{user_id}/import", json=body)
