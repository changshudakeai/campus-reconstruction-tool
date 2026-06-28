import {
  BuildingGeometryProvider,
  confidenceFromSource
} from "./minimalArnisAdapter";
import { createBuildingGeometryObservation } from "../services/buildingObservation";

const fixtureFootprint = [
  { lng: 121.40854, lat: 31.22844 },
  { lng: 121.40903, lat: 31.22858 },
  { lng: 121.40938, lat: 31.2283 },
  { lng: 121.40922, lat: 31.22794 },
  { lng: 121.40874, lat: 31.22785 },
  { lng: 121.40847, lat: 31.22812 }
];

export const putuoLibraryFixtureProvider: BuildingGeometryProvider = {
  source: "overture",
  async fetchBuildingGeometry() {
    return {
      footprint: fixtureFootprint,
      heightM: 22,
      floors: 5,
      roof: {
        shape: "hipped",
        material: "tile",
        orientation: "long_axis"
      },
      facade: {
        material: "stone",
        color: "warm_light"
      },
      confidence: {
        footprint: confidenceFromSource("overture"),
        height: "medium",
        floors: "medium",
        roof: "medium",
        facade: "low"
      },
      observations: [createBuildingGeometryObservation({
        id: "fixture:overture:putuo-library",
        source: "overture",
        sourceFeatureId: "fixture:putuo-library",
        name: "Putuo Campus Library",
        tags: { fixture: "true" },
        components: [{ exterior: fixtureFootprint, interiorRings: [] }],
        normalizationNotes: ["Deterministic offline fixture; never evidence of a live reconstruction."]
      })],
      notes: [
        "Fixture stands in for the first Overture-derived Putuo Campus Library record."
      ]
    };
  }
};
