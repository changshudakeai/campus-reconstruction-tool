use campus_services::acquisition::{AcquisitionClient, HttpsTransport, TransportError};

pub type ProductionAcquisitionClient = AcquisitionClient<HttpsTransport>;

pub fn production_client(
    service_url: impl Into<String>,
    installation_credential: impl Into<String>,
) -> Result<ProductionAcquisitionClient, TransportError> {
    HttpsTransport::new(service_url, installation_credential).map(AcquisitionClient::new)
}

pub fn production_client_if_configured(
    service_url: Option<&str>,
) -> Result<Option<ProductionAcquisitionClient>, String> {
    let Some(service_url) = service_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let credential =
        keyring::Entry::new("CampusReconstructionTool", "acquisition-service-credential")
            .map_err(|error| format!("could not open the acquisition credential store: {error}"))?
            .get_password()
            .map_err(|error| {
                format!("could not read the acquisition service credential: {error}")
            })?;
    production_client(service_url, credential)
        .map(Some)
        .map_err(|error| error.explanation)
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

    #[test]
    fn production_client_is_optional_until_a_service_is_configured() {
        assert!(production_client_if_configured(None).unwrap().is_none());
    }
}
