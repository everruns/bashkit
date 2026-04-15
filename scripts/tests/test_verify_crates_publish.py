"""Lock the verifier to retry on transient crates.io error payloads."""

from __future__ import annotations

import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from verify_crates_publish import (  # noqa: E402
    HttpResponse,
    RegistryResponseError,
    extract_max_version,
    fetch_max_version_with_retries,
)


class VerifyCratesPublishTests(unittest.TestCase):
    def test_extract_max_version_reads_success_payload(self) -> None:
        version = extract_max_version(
            "bashkit",
            HttpResponse(
                status=200,
                body=b'{"crate":{"id":"bashkit","max_version":"0.1.19"}}',
            ),
        )
        self.assertEqual(version, "0.1.19")

    def test_retry_succeeds_after_transient_error_payload(self) -> None:
        responses = iter(
            [
                HttpResponse(
                    status=200,
                    body=b'{"errors":[{"detail":"crate metadata not ready"}]}',
                ),
                HttpResponse(
                    status=200,
                    body=b'{"crate":{"id":"bashkit","max_version":"0.1.19"}}',
                ),
            ]
        )

        version = fetch_max_version_with_retries(
            crate="bashkit",
            attempts=2,
            delay_seconds=0,
            timeout_seconds=1,
            fetcher=lambda crate, timeout: next(responses),
        )

        self.assertEqual(version, "0.1.19")

    def test_retry_failure_reports_last_payload_shape(self) -> None:
        with self.assertRaises(RegistryResponseError) as error:
            fetch_max_version_with_retries(
                crate="bashkit",
                attempts=2,
                delay_seconds=0,
                timeout_seconds=1,
                fetcher=lambda crate, timeout: HttpResponse(
                    status=200,
                    body=b'{"errors":[{"detail":"still propagating"}]}',
                ),
            )

        self.assertIn("missing 'crate' object", str(error.exception))


if __name__ == "__main__":
    unittest.main()
