use std::{sync::Arc, time::Duration};

use libwebrtc::peer_connection_factory::IceServer;
use reqwest::{Client, Method, StatusCode, Url, header};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream,
    tungstenite::{client::IntoClientRequest, http::HeaderValue},
};

use crate::{CfPublishConfig, Error, Result};

#[derive(Clone)]
pub struct Worker {
    client: Client,
    base: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SfuBackend {
    Cloudflare,
    Livekit,
}

#[derive(Deserialize)]
pub struct LiveKitConnection {
    pub url: String,
    pub token: String,
    pub room: String,
    pub identity: String,
}

#[derive(Deserialize)]
pub struct LiveKitStart {
    pub generation: u64,
    pub livekit: LiveKitConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartResponse {
    pub answer: String,
    pub generation: u64,
}

#[derive(Deserialize)]
pub(crate) struct Heartbeat {
    pub ok: bool,
    pub generation: u64,
    #[serde(default)]
    pub keyframe: bool,
}

#[derive(Serialize)]
pub(crate) struct Generation {
    pub generation: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IceResponse {
    ice_servers: Vec<IceEntry>,
}

#[derive(Deserialize)]
struct IceEntry {
    urls: IceUrls,
    #[serde(default)]
    username: String,
    #[serde(default)]
    credential: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum IceUrls {
    One(String),
    Many(Vec<String>),
}

impl Worker {
    pub async fn active_provider(&self) -> Result<SfuBackend> {
        #[derive(Deserialize)]
        struct Response {
            provider: SfuBackend,
        }
        match self.request::<Response>(Method::GET, "/api/sfu", &()).await {
            Ok(value) => Ok(value.provider),
            // Workers anteriores ao contrato são exclusivamente Cloudflare.
            Err(Error::Status {
                status: StatusCode::NOT_FOUND,
                ..
            }) => Ok(SfuBackend::Cloudflare),
            Err(error) => Err(error),
        }
    }

    pub async fn start_livekit(&self, config: &CfPublishConfig) -> Result<LiveKitStart> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Start<'a> {
            providers: [&'static str; 2],
            expected_provider: &'static str,
            channel_id: &'a Option<String>,
            direct_session_id: &'a Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            audio_mid: Option<&'static str>,
        }
        self.request(
            Method::POST,
            "/api/share/start",
            &Start {
                providers: ["cloudflare", "livekit"],
                expected_provider: "livekit",
                channel_id: &config.channel_id,
                direct_session_id: &config.direct_session_id,
                audio_mid: config.audio.then_some("audio"),
            },
        )
        .await
    }

    pub async fn heartbeat(&self, generation: u64) -> Result<bool> {
        let beat: Heartbeat = self
            .request(
                Method::POST,
                "/api/share/heartbeat",
                &Generation { generation },
            )
            .await?;
        if !beat.ok || beat.generation != generation {
            return Err(Error::StaleGeneration);
        }
        Ok(beat.keyframe)
    }

    pub async fn listen_keyframes(
        &self,
        token: String,
        requested: Arc<std::sync::atomic::AtomicBool>,
        shutdown: tokio::sync::watch::Receiver<()>,
    ) {
        crate::events::run_with_callback(
            self.clone(),
            token,
            move || requested.store(true, std::sync::atomic::Ordering::Release),
            shutdown,
        )
        .await;
    }

    pub fn new(config: &CfPublishConfig) -> Result<Self> {
        let url = Url::parse(&config.worker_url).map_err(|_| Error::Config("MADSCREEN_CF_URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Error::Config("MADSCREEN_CF_URL"));
        }
        let mut headers = header::HeaderMap::new();
        let mut auth = header::HeaderValue::from_str(&format!("Bearer {}", config.token))
            .map_err(|_| Error::Config("MADSCREEN_CF_TOKEN"))?;
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);
        let client = Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| Error::Http(e.without_url()))?;
        Ok(Self {
            client,
            base: config.worker_url.trim_end_matches('/').to_owned(),
        })
    }

    pub async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        route: &'static str,
        body: &impl Serialize,
    ) -> Result<T> {
        let request = self
            .client
            .request(method.clone(), format!("{}{route}", self.base));
        let request = if method == Method::GET {
            request
        } else {
            request.json(body)
        };
        let response = request
            .send()
            .await
            .map_err(|e| Error::Http(e.without_url()))?;
        match response.status() {
            StatusCode::CONFLICT => return Err(Error::StaleGeneration),
            status if !status.is_success() => {
                let detail = response
                    .json::<serde_json::Value>()
                    .await
                    .ok()
                    .map(|v| {
                        let field = |k: &str| match v.get(k) {
                            Some(serde_json::Value::String(s)) => s.chars().take(200).collect(),
                            Some(other) if !other.is_null() => other.to_string(),
                            _ => String::new(),
                        };
                        format!(
                            " ({} upstream={} {})",
                            field("error"),
                            field("upstream"),
                            field("detail")
                        )
                    })
                    .unwrap_or_default();
                return Err(Error::Status {
                    route,
                    status,
                    detail,
                });
            }
            _ => {}
        }
        // Não propaga corpos do Worker: podem conter SDP ou informações privadas.
        response.json().await.map_err(|_| Error::Protocol(route))
    }

    pub async fn ice_servers(&self) -> Result<Vec<IceServer>> {
        let response: IceResponse = self.request(Method::GET, "/api/ice", &()).await?;
        Ok(response
            .ice_servers
            .into_iter()
            .map(|entry| IceServer {
                urls: match entry.urls {
                    IceUrls::One(url) => vec![url],
                    IceUrls::Many(urls) => urls,
                },
                username: entry.username,
                password: entry.credential,
            })
            .collect())
    }

    pub async fn keyframe_socket(
        &self,
        token: &str,
    ) -> Result<WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>> {
        #[derive(Deserialize)]
        struct Me {
            user: Owner,
        }
        #[derive(Deserialize)]
        struct Owner {
            id: String,
        }
        // canWatch aceita o ID do próprio usuário e o StreamDO atribui a tag host.
        let me: Me = self.request(Method::GET, "/api/me", &()).await?;
        let mut url = Url::parse(&format!("{}/api/stream/ws", self.base))
            .map_err(|_| Error::Config("MADSCREEN_CF_URL"))?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| Error::Config("MADSCREEN_CF_URL"))?;
        url.query_pairs_mut().append_pair("owner", &me.user.id);
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| Error::Protocol("/api/stream/ws"))?;
        let mut protocol = HeaderValue::from_str(&format!("ms.{token}"))
            .map_err(|_| Error::Config("MADSCREEN_CF_TOKEN"))?;
        protocol.set_sensitive(true);
        request
            .headers_mut()
            .insert("Sec-WebSocket-Protocol", protocol);
        let mut roots = rustls::RootCertStore::empty();
        roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::Protocol("TLS do WebSocket"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        // Erros de handshake podem conter os cabeçalhos: nunca os propaga nem registra.
        // Fixa rustls também quando o LiveKit habilita native-tls na mesma dependência.
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async_tls_with_config(
                request,
                None,
                true,
                Some(Connector::Rustls(Arc::new(tls))),
            ),
        )
        .await
        .map_err(|_| Error::Timeout("WebSocket do Worker"))?
        .map_err(|_| Error::Protocol("/api/stream/ws"))?;
        Ok(socket)
    }

    pub async fn stop(&self, generation: u64) {
        #[derive(Deserialize)]
        struct Stopped {}
        if let Err(error) = self
            .request::<Stopped>(Method::POST, "/api/share/stop", &Generation { generation })
            .await
        {
            tracing::warn!(%error, "não foi possível encerrar a geração no Worker");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, Result};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn endpoint(
        status: u16,
        response: &str,
    ) -> Result<(CfPublishConfig, thread::JoinHandle<Result<String>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let response = response.to_owned();
        let server = thread::spawn(move || -> Result<String> {
            let (mut socket, _) = listener.accept()?;
            socket.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut input = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let read = socket.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                input.extend_from_slice(&chunk[..read]);
                if let Some(end) = input.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&input[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if input.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            write!(
                socket,
                "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )?;
            Ok(String::from_utf8(input)?)
        });
        let config = CfPublishConfig {
            worker_url: format!("http://{address}"),
            token: "test-session".into(),
            ..Default::default()
        };
        Ok((config, server))
    }

    #[test]
    fn provider_selection_preserves_legacy_and_rejects_unknown_or_unauthorized() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        for (status, response, expected) in [
            (
                200,
                r#"{"provider":"cloudflare"}"#,
                Some(SfuBackend::Cloudflare),
            ),
            (200, r#"{"provider":"livekit"}"#, Some(SfuBackend::Livekit)),
            (404, "{}", Some(SfuBackend::Cloudflare)),
            (401, "{}", None),
            (200, r#"{"provider":"unknown"}"#, None),
        ] {
            let (config, server) = endpoint(status, response)?;
            let result = runtime.block_on(async { Worker::new(&config)?.active_provider().await });
            match expected {
                Some(provider) => assert_eq!(result?, provider),
                None => assert!(result.is_err()),
            }
            let request = server
                .join()
                .map_err(|_| anyhow::anyhow!("mock server panicked"))??;
            assert!(request.starts_with("GET /api/sfu "));
        }
        Ok(())
    }

    #[test]
    fn livekit_start_carries_audio_channel_and_provider_guard_without_admin_keys() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (mut config, server) = endpoint(
            200,
            r#"{"generation":7,"livekit":{"url":"wss://media.example.com","token":"join-token","room":"room","identity":"publisher"}}"#,
        )?;
        config.channel_id = Some("channel-a".into());
        config.audio = true;
        let started =
            runtime.block_on(async { Worker::new(&config)?.start_livekit(&config).await })?;
        assert_eq!(started.generation, 7);
        let request = server
            .join()
            .map_err(|_| anyhow::anyhow!("mock server panicked"))??;
        let (_, body) = request
            .split_once("\r\n\r\n")
            .context("mock request body")?;
        let body: serde_json::Value = serde_json::from_str(body)?;
        assert_eq!(body["expectedProvider"], "livekit");
        assert_eq!(body["audioMid"], "audio");
        assert_eq!(body["channelId"], "channel-a");
        assert_eq!(
            body["providers"],
            serde_json::json!(["cloudflare", "livekit"])
        );
        assert!(body.get("apiKey").is_none());
        assert!(body.get("apiSecret").is_none());
        Ok(())
    }

    #[test]
    fn livekit_start_carries_private_session_without_channel() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (mut config, server) = endpoint(
            200,
            r#"{"generation":8,"livekit":{"url":"wss://media.example.com","token":"join-token","room":"room","identity":"publisher"}}"#,
        )?;
        config.direct_session_id = Some("d_private".into());
        runtime.block_on(async { Worker::new(&config)?.start_livekit(&config).await })?;
        let request = server
            .join()
            .map_err(|_| anyhow::anyhow!("mock server panicked"))??;
        let (_, body) = request
            .split_once("\r\n\r\n")
            .context("mock request body")?;
        let body: serde_json::Value = serde_json::from_str(body)?;
        assert_eq!(body["directSessionId"], "d_private");
        assert!(body["channelId"].is_null());
        Ok(())
    }

    #[test]
    fn heartbeat_rejects_a_different_generation() -> Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let (config, server) = endpoint(200, r#"{"ok":true,"generation":8,"keyframe":true}"#)?;
        let result = runtime.block_on(async { Worker::new(&config)?.heartbeat(7).await });
        assert!(matches!(result, Err(Error::StaleGeneration)));
        server
            .join()
            .map_err(|_| anyhow::anyhow!("mock server panicked"))??;
        Ok(())
    }
}
