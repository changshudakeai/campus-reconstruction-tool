import gzip
import hashlib
import json
import unittest
from pathlib import Path

from jsonschema import Draft202012Validator

from services.acquisition_contract import FixtureAcquisitionService, decode_contract_fixture


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

        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)
        self.assertFalse(list(validator.iter_errors(fixture)))
        self.assertTrue(list(validator.iter_errors({})))
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
        canonical_observation = (
            json.dumps(
                observation,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            )
            + "\n"
        ).encode("utf-8")
        canonical_geometry = json.dumps(
            observation["geometry"],
            ensure_ascii=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
        self.assertEqual(
            hashlib.sha256(canonical_geometry).hexdigest(),
            observation["geometry_sha256"],
        )
        self.assertEqual(
            hashlib.sha256(canonical_observation).hexdigest(),
            manifest["chunks"][0]["sha256"],
        )
        self.assertEqual(len(canonical_observation), manifest["chunks"][0]["uncompressed_bytes"])
        self.assertEqual(
            hashlib.sha256(canonical_observation).hexdigest(),
            manifest["result_sha256"],
        )
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
        schema = json.loads(
            (CONTRACT_DIR / "acquisition.schema.json").read_text(encoding="utf-8")
        )
        self.assertFalse(list(Draft202012Validator(schema).iter_errors(snapshot)))
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

    def test_python_service_implements_every_job_control_and_result_route(self) -> None:
        service = FixtureAcquisitionService(FIXTURE_DIR)
        self.assertEqual(service.handle("GET", "/v1/capabilities").status, 200)

        for kind in ("boundary-jobs", "acquisition-jobs"):
            created = service.handle("POST", f"/v1/{kind}", b'{"request_identity":"fixture"}')
            self.assertEqual(created.status, 202)
            job_id = json.loads(created.body)["job_id"]
            self.assertEqual(service.handle("GET", f"/v1/{kind}/{job_id}").status, 200)
            self.assertEqual(
                service.handle("POST", f"/v1/{kind}/{job_id}/retry", b'{"scopes":[]}').status,
                202,
            )
            manifest_response = service.handle(
                "GET", f"/v1/{kind}/{job_id}/manifest"
            )
            self.assertEqual(manifest_response.status, 200)
            manifest = json.loads(manifest_response.body)
            chunk = manifest["chunks"][0]
            chunk_response = service.handle(
                "GET",
                f"/v1/{kind}/{job_id}/chunks/{chunk['id']}",
                cursor=chunk["stable_cursor"],
            )
            self.assertEqual(chunk_response.status, 200)
            self.assertEqual(
                chunk_response.headers["x-stable-cursor"], chunk["stable_cursor"]
            )
            decoded_chunk = gzip.decompress(chunk_response.body)
            self.assertEqual(hashlib.sha256(decoded_chunk).hexdigest(), chunk["sha256"])
            self.assertEqual(len(decoded_chunk), chunk["uncompressed_bytes"])
            self.assertEqual(
                service.handle("POST", f"/v1/{kind}/{job_id}/cancel").status,
                202,
            )

        failure = service.handle("GET", "/v1/acquisition-jobs/missing")
        self.assertEqual(failure.status, 404)
        self.assertIn("suggested_action", json.loads(failure.body))


if __name__ == "__main__":
    unittest.main()
