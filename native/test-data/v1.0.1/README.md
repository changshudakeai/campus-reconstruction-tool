# V1.0.1 compatibility baseline

These fixtures freeze the accepted native V1.0.1 compatibility boundary before schema-2 work.

- `../v1-demo.campus.json` is a populated native schema-1 project. It contains every persisted top-level project field and every persisted Detailed Building collection. Regenerable output paths are deliberately null or relative.
- `legacy-web-portable-project.json` exercises the supported legacy web Portable Project decoder, including candidate reviews, manual geometry, campus names, suppressions, style, scale, and orientation.
- `regression-contract.json` versions the expected Foundation, Detailed, provider, coordinate, generator, Sponge schematic, helper-process, and deployment seams.
- `failures/` freezes corrupt, truncated, partial-write, unsafe-relative-path, and injected-provider-failure inputs. Fixture values are synthetic and contain no credentials or personal machine paths.

Baseline commands:

```powershell
cargo +stable check --manifest-path native/Cargo.toml --workspace
cargo +stable test --manifest-path native/Cargo.toml --workspace
python -m unittest services/test_deployment_contract.py
```
