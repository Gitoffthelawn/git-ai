#[test]
fn test_https_request_uses_native_certs() {
    let response = minreq::get("https://gitlab.com/api/v4/version")
        .with_timeout(10)
        .send();

    match response {
        Ok(resp) => {
            assert_ne!(
                resp.status_code, 0,
                "Expected a valid HTTP status code from GitLab"
            );
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                !err_str.contains("UnknownIssuer"),
                "TLS should use native system certificates, but got UnknownIssuer error: {}",
                err_str
            );
            assert!(
                !err_str.contains("invalid peer certificate"),
                "TLS should use native system certificates, but got certificate error: {}",
                err_str
            );
        }
    }
}

#[test]
fn test_https_request_to_public_endpoint() {
    let response = minreq::get("https://httpbin.org/get")
        .with_timeout(10)
        .send();

    match response {
        Ok(resp) => {
            assert_eq!(resp.status_code, 200, "Expected 200 from httpbin.org");
        }
        Err(e) => {
            let err_str = e.to_string();
            assert!(
                !err_str.contains("UnknownIssuer") && !err_str.contains("invalid peer certificate"),
                "TLS certificate validation failed unexpectedly: {}",
                err_str
            );
        }
    }
}

#[test]
fn test_native_cert_store_is_loaded() {
    let certs = rustls_native_certs::load_native_certs()
        .expect("Should be able to load native certificate store");
    assert!(
        !certs.is_empty(),
        "Native certificate store should contain at least one certificate"
    );
}

#[test]
fn test_minreq_https_does_not_reject_valid_certs() {
    let urls = vec![
        "https://google.com",
        "https://github.com",
        "https://gitlab.com",
    ];

    for url in urls {
        let response = minreq::get(url).with_timeout(10).send();

        if let Err(e) = &response {
            let err_str = e.to_string();
            assert!(
                !err_str.contains("UnknownIssuer")
                    && !err_str.contains("invalid peer certificate")
                    && !err_str.contains("CertNotValidForName"),
                "TLS validation should not reject valid cert for {}: {}",
                url,
                err_str
            );
        }
    }
}
