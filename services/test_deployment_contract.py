import ast
import json
import re
import unittest
from pathlib import Path


SERVICE_DIR = Path(__file__).parent
ROOT_DIR = SERVICE_DIR.parent


class DeploymentContractTests(unittest.TestCase):
    def test_docker_environment_matches_bridge_configuration(self) -> None:
        source = (SERVICE_DIR / "overture_bridge.py").read_text(encoding="utf-8")
        tree = ast.parse(source)
        requested = {
            node.args[0].value
            for node in ast.walk(tree)
            if isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and isinstance(node.func.value, ast.Name)
            and node.func.value.id == "os"
            and node.func.attr == "getenv"
            and node.args
            and isinstance(node.args[0], ast.Constant)
            and isinstance(node.args[0].value, str)
            and node.args[0].value.startswith("OVERTURE_BRIDGE_")
        }
        dockerfile = (SERVICE_DIR / "Dockerfile").read_text(encoding="utf-8")
        configured = set(re.findall(r"\b(OVERTURE_[A-Z_]+)=", dockerfile))

        self.assertTrue(requested)
        self.assertTrue(
            requested.issubset(configured),
            f"Dockerfile is missing bridge variables: {sorted(requested - configured)}",
        )

    def test_windows_installer_uses_the_application_version(self) -> None:
        package_version = json.loads(
            (ROOT_DIR / "package.json").read_text(encoding="utf-8")
        )["version"]
        installer = (ROOT_DIR / "installer" / "campus-reconstruction-tool.nsi").read_text(
            encoding="utf-8"
        )
        match = re.search(r'!define PRODUCT_VERSION "([^"]+)"', installer)

        self.assertIsNotNone(match)
        self.assertEqual(match.group(1), package_version)


if __name__ == "__main__":
    unittest.main()
