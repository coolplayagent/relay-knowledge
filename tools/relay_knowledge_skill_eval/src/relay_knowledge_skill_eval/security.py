from __future__ import annotations

import re

_KEY_PATTERN = re.compile(r"(?i)(?:bearer\s+)?sk-[A-Za-z0-9_-]{16,}")


class SecretRedactor:
    def __init__(self, secret: str = "") -> None:
        self._secret = secret

    def redact(self, value: str) -> str:
        redacted = value.replace(self._secret, "[REDACTED]") if self._secret else value
        return _KEY_PATTERN.sub("[REDACTED]", redacted)
