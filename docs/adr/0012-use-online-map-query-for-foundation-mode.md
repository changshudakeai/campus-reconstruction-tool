# Use Online Map Query for Foundation Mode

Foundation Mode will prioritize Online Map Query through the user's Gaode API access instead of starting from cached sample files. Cached data can still support debugging and regression tests, but the product path should use live map services because the existing cache quality is not reliable enough for the intended campus reconstruction workflow.

User-provided Gaode keys may be stored in local application configuration and override build-time environment keys for POI search, reverse geocoding, and Gaode JS map rendering. This lets different campus users bring their own quota without rebuilding the app.
