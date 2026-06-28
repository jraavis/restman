//! gRPC transport over HTTP/2 + TLS (#25 fills in the production `connect()`;
//! this file lands in #23 with the offline TLS-stack validation test only).
//!
//! Why a hand-rolled h2 client at all: reqwest (0.12) doesn't expose raw
//! HTTP/2 trailers, which gRPC carries `grpc-status` in. So gRPC speaks h2
//! directly via the `h2` crate, over a `tokio-rustls` TLS session with ALPN
//! negotiating `h2`. The TLS stack is **rustls** (not native-tls) — chosen in
//! #23 for clean ALPN and to keep one TLS implementation in the binary (after
//! reqwest was tightened to `rustls-tls-native-roots`), and `aws-lc-rs` is the
//! crypto provider (rustls 0.23 default; already a transitive dep of the
//! tightened reqwest, so the test below doesn't add a new TLS impl).

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
    use rustls::server::ServerConfig;
    use rustls::{ClientConfig, RootCertStore};

    /// Self-signed cert with SAN `localhost`, via `rcgen` (dev-only dep, never
    /// shipped). ECDSA P-256 + SHA-256 so it's verifiable by the `aws-lc-rs`
    /// provider used below (standard curve — both `ring`-signed certs and
    /// `aws-lc-rs` verification interop).
    fn self_signed_cert() -> (CertificateDer<'static>, PrivateKeyDer<'static>) {
        let params =
            rcgen::CertificateParams::new(vec!["localhost".to_string()])
                .expect("rcgen: subject alt names");
        let key_pair = rcgen::KeyPair::generate().expect("rcgen: generate keypair");
        let cert = params
            .self_signed(&key_pair)
            .expect("rcgen: self-signed cert");
        let cert_der = cert.der().clone();
        // rcgen 0.13 emits PKCS#8 by default.
        let key = PrivateKeyDer::Pkcs8(key_pair.serialize_der().to_vec().into());
        (cert_der, key)
    }

    fn client_config(root: CertificateDer<'static>) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(root).expect("add self-signed cert as trusted root");
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let mut cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls: safe protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        // `with_no_client_auth` finalizes the builder — ALPN is set on the
        // finished config's public field rather than via a terminal builder
        // method (rustls 0.23 has no `with_alpn_protocols` on the finalized
        // `ClientConfig`).
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        Arc::new(cfg)
    }

    fn server_config(
        cert: CertificateDer<'static>,
        key: PrivateKeyDer<'static>,
    ) -> Arc<ServerConfig> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let mut cfg = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("rustls: safe protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("rustls: load self-signed cert/key");
        cfg.alpn_protocols = vec![b"h2".to_vec()];
        Arc::new(cfg)
    }

    /// Offline proof the chosen TLS stack (#23) composes: a loopback TCP
    /// socket, a `tokio-rustls` server + client, both pinned to ALPN `h2`, and
    /// the negotiated ALPN protocol comes out as `h2`. The HTTP/2 frame
    /// round-trip on top of this session is #25's job (transport KAT); this
    /// isolates the riskiest *new* surface (rustls + ALPN + provider wiring,
    /// none of which this repo had before 17d) so the stack decision gates
    /// nothing downstream on a surprise.
    #[tokio::test(flavor = "current_thread")]
    async fn loopback_tls_handshake_negotiates_alpn_h2() {
        let (cert, key) = self_signed_cert();
        let server_cfg = server_config(cert.clone(), key);
        let client_cfg = client_config(cert);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let server = tokio::spawn(async move {
            let (sock, _) = listener.accept().await.expect("accept");
            let acceptor = TlsAcceptor::from(server_cfg);
            let mut tls = acceptor.accept(sock).await.expect("server tls handshake");
            // ALPN negotiated by the server side.
            let (_, server_session) = tls.get_ref();
            assert_eq!(
                server_session.alpn_protocol(),
                Some(b"h2".as_slice()),
                "server should have ALPN-negotiated h2"
            );
            // Echo a byte back so the client's read path also proves the
            // session is bidirectionally usable, not just established.
            tls.write_all(b"hello-h2").await.expect("server write");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            tls.shutdown().await.ok();
        });

        let sock = TcpStream::connect(addr).await.expect("client tcp connect");
        let connector = TlsConnector::from(client_cfg);
        let domain = ServerName::try_from("localhost").expect("server name");
        let mut tls = connector.connect(domain, sock).await.expect("client tls handshake");
        let (_, client_session) = tls.get_ref();
        assert_eq!(
            client_session.alpn_protocol(),
            Some(b"h2".as_slice()),
            "client should have ALPN-negotiated h2"
        );

        // Drain the server's echoed byte to confirm the session carries data.
        let mut buf = [0u8; 8];
        tls.read_exact(&mut buf).await.expect("client read");
        assert_eq!(&buf, b"hello-h2");

        server.await.expect("server task did not finish cleanly");
    }
}