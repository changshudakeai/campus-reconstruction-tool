use campus_services::acquisition::{AcquisitionClient, HttpsTransport, TransportError};

pub type ProductionAcquisitionClient = AcquisitionClient<HttpsTransport>;

pub fn production_client(
    service_url: impl Into<String>,
    installation_credential: impl Into<String>,
) -> Result<ProductionAcquisitionClient, TransportError> {
    HttpsTransport::new(service_url, installation_credential).map(AcquisitionClient::new)
}

#[cfg(debug_assertions)]
pub fn bootstrap_fixture_if_enabled(
    is_debug_build: bool,
    enabled: Option<&str>,
) -> Result<Option<(usize, usize)>, String> {
    use campus_services::acquisition::fixture_transport::FixtureTransport;

    if !is_debug_build || enabled != Some("1") {
        return Ok(None);
    }
    let client = AcquisitionClient::new(FixtureTransport::canonical()?);
    let boundary = client
        .load_boundary_discovery("desktop-fixture-boundary")
        .map_err(|error| error.to_string())?;
    let acquisition = client
        .load_acquisition_result("desktop-fixture-acquisition")
        .map_err(|error| error.to_string())?;
    Ok(Some((
        boundary.candidates.len(),
        acquisition.observations.len(),
    )))
}

#[cfg(not(debug_assertions))]
pub fn bootstrap_fixture_if_enabled(
    _is_debug_build: bool,
    _enabled: Option<&str>,
) -> Result<Option<(usize, usize)>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_development_path_consumes_both_fixed_results() {
        assert_eq!(
            bootstrap_fixture_if_enabled(true, Some("1")).unwrap(),
            Some((2, 1))
        );
    }

    #[test]
    fn production_client_rejects_plaintext_remote_transport() {
        assert!(production_client("http://example.com", "secret").is_err());
    }
}
