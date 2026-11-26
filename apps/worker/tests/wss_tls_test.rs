//! End-to-end proof of the mandatory verified-WSS path against a real in-process
//! TLS + WebSocket Hub stub whose certificate is generated with `openssl`: an
//! unknown chain fails closed, `MYRLET_HUB_CA_FILE` is honoured, and both a
//! hostname mismatch and an unusable CA bundle still fail closed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use myrlet_protocol::{WorkerId, WorkerWireEnvelope, WorkerWireKind, WORKER_PROTOCOL_VERSION};
use myrlet_worker_daemon::tls::TlsError;
use myrlet_worker_daemon::wss::{self, WssError};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::handshake::server::{
    Callback, ErrorResponse, Request, Response,
};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

const CREDENTIAL: &str = "tls-test-cred";
const CONNECT_PATH: &str = "/v1/workers/connect";
const EPOCH: u64 = 7;
const TIMEOUT: Duration = Duration::from_secs(60);

struct Pki {
    cert: PathBuf,
    key: PathBuf,
}

#[tokio::test]
async fn wss_rejects_untrusted_cert_and_succeeds_with_custom_ca_bundle() {
    tokio::time::timeout(TIMEOUT, verify_wss_tls())
        .await
        .expect("the WSS TLS test timed out");
}

async fn verify_wss_tls() {
    let dir = tempfile::tempdir().expect("create the temporary PKI directory");
    let trusted = issue_cert(dir.path(), "trusted", "DNS:localhost,IP:127.0.0.1");
    let (hub_url, mut observed, _hub) = spawn_hub(&trusted).await;

    // Case 1: the default Mozilla-roots client must reject the unknown chain.
    let default_tls = myrlet_worker_daemon::build_tls_config(None).expect("default TLS config");
    match wss::connect(&hub_url, CREDENTIAL, &default_tls).await {
        Err(WssError::Connect { .. }) => {}
        Err(error) => panic!("expected certificate verification to fail, got {error}"),
        Ok(_) => panic!("the untrusted test certificate must be rejected"),
    }

    // Case 2: the same certificate is trusted once supplied as a CA bundle.
    let custom_tls = myrlet_worker_daemon::build_tls_config(Some(trusted.cert.as_path()))
        .expect("trust the test certificate");
    let socket = wss::connect(&hub_url, CREDENTIAL, &custom_tls)
        .await
        .expect("a CA added through MYRLET_HUB_CA_FILE must be trusted");
    let (mut sink, mut source) = wss::split(socket);
    let hello = hello_envelope(WorkerId::new("wkr_0000000000TLS"));
    wss::send(&mut sink, &hello)
        .await
        .expect("send worker.hello over the verified channel");
    let reply = wss::next(&mut source).await.expect("read the Hub reply");
    let reply = reply.expect("the Hub replies before closing");
    assert_eq!(reply.kind, WorkerWireKind::HubWelcome);
    assert_eq!(reply.connection_epoch, Some(EPOCH));
    assert_eq!(reply.worker_id, hello.worker_id);
    assert_eq!(
        observed.recv().await.expect("the Hub saw the upgrade"),
        WorkerWireKind::WorkerHello
    );

    // Case 3: a bundle-trusted certificate issued for another name still fails.
    let wrong = issue_cert(dir.path(), "mismatched", "DNS:hub.invalid");
    let wrong_tls = myrlet_worker_daemon::build_tls_config(Some(wrong.cert.as_path()))
        .expect("trust the mismatched certificate");
    let (wrong_url, _observed, _wrong_hub) = spawn_hub(&wrong).await;
    match wss::connect(&wrong_url, CREDENTIAL, &wrong_tls).await {
        Err(WssError::Connect { .. }) => {}
        Err(error) => panic!("expected an IP/hostname mismatch, got {error}"),
        Ok(_) => panic!("127.0.0.1 must not match a DNS-only certificate"),
    }

    // Case 4: an unusable CA bundle fails closed before any network activity.
    let empty = dir.path().join("empty-ca.pem");
    std::fs::write(&empty, b"").expect("write the empty CA bundle");
    match myrlet_worker_daemon::build_tls_config(Some(empty.as_path())) {
        Err(TlsError::CaBundle { .. }) => {}
        Err(error) => panic!("expected an unusable CA bundle, got {error}"),
        Ok(_) => panic!("an empty CA bundle must not be trusted"),
    }
}

fn hello_envelope(worker_id: WorkerId) -> WorkerWireEnvelope<Value> {
    WorkerWireEnvelope {
        v: WORKER_PROTOCOL_VERSION.to_owned(),
        kind: WorkerWireKind::WorkerHello,
        message_id: "msg_worker_hello".to_owned(),
        worker_id,
        slot_id: None,
        connection_id: None,
        connection_epoch: None,
        sent_at_ms: 0,
        reply_to: None,
        payload: json!({}),
    }
}

/// The leaf is self-signed and marked `CA:FALSE`: rustls-webpki rejects a CA
/// certificate used as the end entity, while a self-signed leaf trusted directly
/// as the root is exactly what an operator CA bundle contains.
fn issue_cert(root: &Path, name: &str, san: &str) -> Pki {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("create the certificate directory");
    let san = format!("subjectAltName={san}");
    openssl(
        &dir,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "key.pem",
            "-out",
            "cert.pem",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            san.as_str(),
            "-addext",
            "basicConstraints=critical,CA:FALSE",
        ],
    );
    Pki {
        cert: dir.join("cert.pem"),
        key: dir.join("key.pem"),
    }
}

fn openssl(dir: &Path, args: &[&str]) {
    let output = Command::new("openssl")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("openssl must be installed to run this test");
    assert!(
        output.status.success(),
        "openssl {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn server_config(cert: &Path, key: &Path) -> rustls::ServerConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certificates = CertificateDer::pem_file_iter(cert)
        .expect("read the server certificate")
        .collect::<Result<Vec<_>, _>>()
        .expect("parse the server certificate");
    let key = PrivateKeyDer::from_pem_file(key).expect("read the server key");
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("TLS 1.2/1.3 protocol versions")
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .expect("install the server certificate")
}

async fn spawn_hub(pki: &Pki) -> (String, UnboundedReceiver<WorkerWireKind>, JoinHandle<()>) {
    let acceptor = TlsAcceptor::from(Arc::new(server_config(&pki.cert, &pki.key)));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the loopback listener");
    let port = listener
        .local_addr()
        .expect("read the loopback address")
        .port();
    let (observed, receiver) = mpsc::unbounded_channel();
    let hub = tokio::spawn(async move {
        loop {
            let Ok((tcp, _peer)) = listener.accept().await else {
                continue;
            };
            let acceptor = acceptor.clone();
            let observed = observed.clone();
            tokio::spawn(async move {
                // A client that rejects our chain aborts the handshake here; the
                // listener keeps accepting so later cases can still connect.
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(tls, Authorize).await
                else {
                    return;
                };
                serve(&mut socket, observed).await;
            });
        }
    });
    (format!("https://127.0.0.1:{port}"), receiver, hub)
}

struct Authorize;

impl Callback for Authorize {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let expected = format!("Bearer {CREDENTIAL}");
        let bearer = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok());
        if request.uri().path() == CONNECT_PATH && bearer == Some(expected.as_str()) {
            return Ok(response);
        }
        let mut denied = ErrorResponse::new(Some("wrong worker credential".into()));
        *denied.status_mut() = StatusCode::UNAUTHORIZED;
        Err(denied)
    }
}

async fn serve<S>(socket: &mut WebSocketStream<S>, observed: UnboundedSender<WorkerWireKind>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        return;
    };
    let Ok(request) = serde_json::from_str::<WorkerWireEnvelope<Value>>(&text) else {
        return;
    };
    let _ = observed.send(request.kind);
    if request.kind != WorkerWireKind::WorkerHello {
        return;
    }
    let reply = WorkerWireEnvelope {
        v: WORKER_PROTOCOL_VERSION.to_owned(),
        kind: WorkerWireKind::HubWelcome,
        message_id: "msg_hub_welcome".to_owned(),
        worker_id: request.worker_id,
        slot_id: None,
        connection_id: Some("conn_tls_test".to_owned()),
        connection_epoch: Some(EPOCH),
        sent_at_ms: 0,
        reply_to: Some(request.message_id),
        payload: json!({
            "hubId": "hub_tls_test",
            "connectionId": "conn_tls_test",
            "connectionEpoch": EPOCH,
            "heartbeatIntervalMs": 5_000,
            "leaseTtlMs": 15_000,
        }),
    };
    let Ok(text) = serde_json::to_string(&reply) else {
        return;
    };
    if socket.send(Message::Text(text)).await.is_ok() {
        let _ = socket.close(None).await;
    }
}
