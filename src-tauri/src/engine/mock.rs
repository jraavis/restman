//! Local mock-server hosting spike. Nothing here is wired into any command
//! yet — this proves the one genuinely new mechanism (a real axum server on
//! a real loopback socket) before the rule-matching engine gets built on top
//! of it. `#[cfg(test)]`-only for now, same posture as the gRPC feasibility
//! spike (17c): a normal `cargo build` compiles this file to nothing. The
//! next step re-homes `serve()`/`RunningMockServer` into real module code
//! once the rule-matching `Router` construction lands alongside it.

#[cfg(test)]
mod tests {
    use axum::routing::get;
    use axum::Router;
    use tokio::net::TcpListener;

    /// A running mock server: the port the OS assigned (port 0 on bind), and
    /// a handle to abort the serve loop on stop. Bound to `127.0.0.1` only,
    /// never `0.0.0.0` — this is a privacy-first app, a mock server is a
    /// local dev tool, not something meant to be reachable from the LAN.
    struct RunningMockServer {
        addr: std::net::SocketAddr,
        handle: tokio::task::JoinHandle<()>,
    }

    impl RunningMockServer {
        fn abort(&self) {
            self.handle.abort();
        }
    }

    async fn serve(router: Router) -> std::io::Result<RunningMockServer> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(RunningMockServer { addr, handle })
    }

    /// Loopback isn't "the internet" — this sandbox's cargo test can reach
    /// 127.0.0.1 even though it can't reach a real external host, so this is
    /// a genuine end-to-end proof, not a fixture-only offline check.
    #[tokio::test]
    async fn serves_a_real_response_over_a_real_loopback_socket() {
        let router = Router::new().route("/ping", get(|| async { "pong" }));
        let server = serve(router).await.unwrap();

        let resp = reqwest::get(format!("http://{}/ping", server.addr)).await.unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "pong");

        server.abort();
    }

    #[tokio::test]
    async fn only_ever_binds_loopback_not_all_interfaces() {
        let router = Router::new();
        let server = serve(router).await.unwrap();
        assert!(server.addr.ip().is_loopback());
        server.abort();
    }
}
