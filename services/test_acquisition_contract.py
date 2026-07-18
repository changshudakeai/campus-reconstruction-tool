import hashlib
import json
import unittest
from pathlib import Path

from services.acquisition_contract import decode_contract_fixture


ROOT_DIR = Path(__file__).parent.parent
CONTRACT_DIR = ROOT_DIR / "contracts" / "acquisition" / "v1"
FIXTURE_DIR = CONTRACT_DIR / "fixtures"


def canonical_sha256(value: object) -> str:
    encoded = json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


class AcquisitionContractTests(unittest.TestCase):
    def test_openapi_freezes_the_complete_v1_surface(self) -> None:
        openapi = json.loads((CONTRACT_DIR / "openapi.json").read_text(encoding="utf-8"))
        expected_operations = {
            ("get", "/v1/health"),
            ("get", "/v1/capabilities"),
            ("post", "/v1/boundary-jobs"),
            ("get", "/v1/boundary-jobs/{job_id}"),
            ("post", "/v1/boundary-jobs/{job_id}/retry"),
            ("post", "/v1/boundary-jobs/{job_id}/cancel"),
            ("get", "/v1/boundary-jobs/{job_id}/manifest"),
            ("get", "/v1/boundary-jobs/{job_id}/chunks/{chunk_id}"),
            ("post", "/v1/acquisition-jobs"),
            ("get", "/v1/acquisition-jobs/{job_id}"),
            ("post", "/v1/acquisition-jobs/{job_id}/retry"),
            ("post", "/v1/acquisition-jobs/{job_id}/cancel"),
            ("get", "/v1/acquisition-jobs/{job_id}/manifest"),
            ("get", "/v1/acquisition-jobs/{job_id}/chunks/{chunk_id}"),
        }
        actual_operations = {
            (method, path)
            for path, methods in openapi["paths"].items()
            for method in methods
            if method in {"get", "post", "put", "patch", "delete"}
        }
        self.assertEqual(actual_operations, expected_operations)
        self.assertEqual(openapi["info"]["version"], "1.0.0")

    def test_shared_fixtures_cover_contract_semantics_and_replay(self) -> None:
        schema = json.loads(
            (CONTRACT_DIR / "acquisition.schema.json").read_text(encoding="utf-8")
        )
        fixture = json.loads(
            (FIXTURE_DIR / "canonical-acquisition.json").read_text(encoding="utf-8")
        )
        replay = json.loads(
            (FIXTURE_DIR / "canonical-acquisition-replay.json").read_text(
                encoding="utf-8"
            )
        )

        self.assertEqual(schema["$id"], "https://contracts.mcrebuild.invalid/v1/acquisition.schema.json")
        self.assertEqual(canonical_sha256(fixture), canonical_sha256(replay))
        self.assertEqual(
            {outcome["status"] for outcome in fixture["coverage_report"]["outcomes"]},
            {"complete", "complete-empty", "partial", "failed", "cancelled"},
        )
        self.assertEqual(
            {outcome["category"] for outcome in fixture["coverage_report"]["outcomes"]},
            {"building", "circulation", "water", "vegetation", "sports"},
        )
        self.assertTrue(
            all("pagination_exhausted" in outcome for outcome in fixture["coverage_report"]["outcomes"])
        )
        self.assertTrue(
            any(not outcome["pagination_exhausted"] for outcome in fixture["coverage_report"]["outcomes"])
        )

        observation = fixture["observations"][0]
        self.assertEqual(observation["geometry"]["type"], "MultiPolygon")
        self.assertTrue(observation["geometry"]["coordinates"][0][0][1])
        self.assertEqual(observation["lineage"]["relation"]["assembly_status"], "complete")
        self.assertTrue(observation["lineage"]["relation"]["member_ids"])
        self.assertEqual(observation["coordinate_semantics"]["crs"], "OGC:CRS84")
        self.assertEqual(observation["unit_semantics"]["height"], "m")
        self.assertTrue(observation["time_semantics"]["dataset_release"])
        self.assertTrue(observation["licence"]["identifier"])

        manifest = fixture["manifest"]
        self.assertRegex(manifest["result_sha256"], r"^[0-9a-f]{64}$")
        self.assertTrue(manifest["chunks"])
        self.assertTrue(all(chunk["stable_cursor"] for chunk in manifest["chunks"]))
        self.assertTrue(
            all(len(chunk["sha256"]) == 64 for chunk in manifest["chunks"])
        )

        decoded = decode_contract_fixture(fixture)
        self.assertEqual(decoded.bundle_id, "cn-campus-2026-06")
        self.assertEqual(len(decoded.observations), 1)
        self.assertEqual(
            [outcome.is_complete for outcome in decoded.outcomes],
            [True, True, False, False, False],
        )

    def test_boundary_snapshot_preserves_ranked_complete_relation_candidates(self) -> None:
        snapshot = json.loads(
            (FIXTURE_DIR / "boundary-discovery-snapshot.json").read_text(
                encoding="utf-8"
            )
        )

        self.assertEqual(snapshot["contract_version"], "1.0.0")
        self.assertEqual(snapshot["bundle"]["id"], "cn-campus-2026-06")
        self.assertGreaterEqual(len(snapshot["candidates"]), 2)
        self.assertEqual(
            [candidate["rank"] for candidate in snapshot["candidates"]], [1, 2]
        )
        self.assertTrue(
            all(
                candidate["lineage"]["relation"]["assembly_status"] == "complete"
                for candidate in snapshot["candidates"]
            )
        )
        self.assertTrue(
            all(candidate["geometry"]["type"] in {"Polygon", "MultiPolygon"} for candidate in snapshot["candidates"])
        )


if __name__ == "__main__":
    unittest.main()
