"""Provider-neutral decoder for the frozen Controlled Acquisition v1 contract."""

from dataclasses import dataclass
from typing import Any, Mapping


CONTRACT_VERSION = "1.0.0"
COMPLETE_STATUSES = frozenset({"complete", "complete-empty"})
KNOWN_STATUSES = COMPLETE_STATUSES | {"partial", "failed", "cancelled"}
KNOWN_CATEGORIES = frozenset(
    {"building", "circulation", "water", "vegetation", "sports"}
)
KNOWN_GEOMETRIES = frozenset(
    {"Point", "MultiPoint", "LineString", "MultiLineString", "Polygon", "MultiPolygon"}
)


class ContractDecodeError(ValueError):
    pass


@dataclass(frozen=True)
class ProviderOutcome:
    provider: str
    category: str
    tile_id: str
    status: str
    pagination_exhausted: bool
    relation_members_complete: bool
    raw_count: int
    deduplicated_count: int

    @property
    def is_complete(self) -> bool:
        return (
            self.status in COMPLETE_STATUSES
            and self.pagination_exhausted
            and self.relation_members_complete
        )


@dataclass(frozen=True)
class SourceObservation:
    observation_id: str
    category: str
    geometry_type: str
    provider: str
    source_record_id: str
    licence_identifier: str
    coordinate_reference_system: str


@dataclass(frozen=True)
class AcquisitionFixture:
    contract_version: str
    bundle_id: str
    outcomes: tuple[ProviderOutcome, ...]
    observations: tuple[SourceObservation, ...]


def _mapping(value: Any, field: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise ContractDecodeError(f"{field} must be an object")
    return value


def _text(value: Any, field: str) -> str:
    if not isinstance(value, str) or not value:
        raise ContractDecodeError(f"{field} must be a non-empty string")
    return value


def decode_contract_fixture(value: Mapping[str, Any]) -> AcquisitionFixture:
    contract_version = _text(value.get("contract_version"), "contract_version")
    if contract_version != CONTRACT_VERSION:
        raise ContractDecodeError(
            f"unsupported contract {contract_version}; expected {CONTRACT_VERSION}"
        )
    bundle = _mapping(value.get("bundle"), "bundle")
    coverage = _mapping(value.get("coverage_report"), "coverage_report")

    outcomes = []
    for index, item in enumerate(coverage.get("outcomes", ())):
        outcome = _mapping(item, f"coverage_report.outcomes[{index}]")
        status = _text(outcome.get("status"), "status")
        category = _text(outcome.get("category"), "category")
        if status not in KNOWN_STATUSES:
            raise ContractDecodeError(f"unknown provider outcome status: {status}")
        if category not in KNOWN_CATEGORIES:
            raise ContractDecodeError(f"unknown Foundation category: {category}")
        outcomes.append(
            ProviderOutcome(
                provider=_text(outcome.get("provider"), "provider"),
                category=category,
                tile_id=_text(outcome.get("tile_id"), "tile_id"),
                status=status,
                pagination_exhausted=outcome.get("pagination_exhausted") is True,
                relation_members_complete=outcome.get("relation_members_complete")
                is True,
                raw_count=int(outcome.get("raw_count", 0)),
                deduplicated_count=int(outcome.get("deduplicated_count", 0)),
            )
        )

    observations = []
    for index, item in enumerate(value.get("observations", ())):
        observation = _mapping(item, f"observations[{index}]")
        geometry = _mapping(observation.get("geometry"), "geometry")
        lineage = _mapping(observation.get("lineage"), "lineage")
        licence = _mapping(observation.get("licence"), "licence")
        coordinates = _mapping(
            observation.get("coordinate_semantics"), "coordinate_semantics"
        )
        geometry_type = _text(geometry.get("type"), "geometry.type")
        if geometry_type not in KNOWN_GEOMETRIES:
            raise ContractDecodeError(f"unknown typed geometry: {geometry_type}")
        observations.append(
            SourceObservation(
                observation_id=_text(observation.get("id"), "observation.id"),
                category=_text(observation.get("category"), "observation.category"),
                geometry_type=geometry_type,
                provider=_text(lineage.get("provider"), "lineage.provider"),
                source_record_id=_text(
                    lineage.get("source_record_id"), "lineage.source_record_id"
                ),
                licence_identifier=_text(
                    licence.get("identifier"), "licence.identifier"
                ),
                coordinate_reference_system=_text(
                    coordinates.get("crs"), "coordinate_semantics.crs"
                ),
            )
        )

    return AcquisitionFixture(
        contract_version=contract_version,
        bundle_id=_text(bundle.get("id"), "bundle.id"),
        outcomes=tuple(outcomes),
        observations=tuple(observations),
    )
