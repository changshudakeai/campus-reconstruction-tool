/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_GAODE_WEB_SERVICE_KEY?: string;
  readonly VITE_GAODE_PROVIDER?: string;
  readonly VITE_GAODE_SECURITY_JS_CODE?: string;
  readonly VITE_GAODE_POI_ENDPOINT?: string;
  readonly VITE_OVERTURE_BUILDING_ENDPOINT?: string;
  readonly VITE_OVERPASS_ENDPOINT?: string;
  readonly VITE_USE_LEGACY_DETAILED_GENERATOR?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
