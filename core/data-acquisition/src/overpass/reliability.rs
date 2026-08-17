//! Overpass 公共实例的有界、顺序可靠性策略。
//!
//! 只在单次用户操作内维护节点健康；不并发请求公共节点，不把失败节点永久拉黑。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{OVERPASS_ENDPOINTS, USER_AGENT};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OverpassRequest {
    pub(super) endpoint: &'static str,
    pub(super) body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FailureKind {
    Timeout,
    Connection,
    RateLimited,
    GatewayTimeout,
    Server,
    ErrorBody,
    Parse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RequestFailure {
    pub(super) kind: FailureKind,
    pub(super) message: String,
}

pub(super) type RequestTransport =
    Arc<dyn Fn(&OverpassRequest, Duration) -> Result<String, RequestFailure> + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub(super) struct RetryPolicy {
    pub(super) request_timeout: Duration,
    pub(super) max_rounds: u8,
    pub(super) retry_backoff: Duration,
    pub(super) transient_cooldown: Duration,
    pub(super) overloaded_cooldown: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct EndpointHealth {
    failures: u32,
    successes: u32,
    cooldown_until: Option<Instant>,
}

#[derive(Debug, Default)]
pub(super) struct RunHealth {
    endpoints: BTreeMap<&'static str, EndpointHealth>,
}

impl RunHealth {
    fn ordered(&self, preferred: &[&'static str]) -> Vec<&'static str> {
        let now = Instant::now();
        let preferred_rank = |endpoint: &&'static str| {
            preferred
                .iter()
                .position(|candidate| candidate == endpoint)
                .unwrap_or(usize::MAX)
        };
        let mut endpoints = OVERPASS_ENDPOINTS.to_vec();
        endpoints.sort_by_key(|endpoint| {
            let health = self.endpoints.get(endpoint).copied().unwrap_or_default();
            let cooling = health.cooldown_until.is_some_and(|until| until > now);
            (
                cooling,
                std::cmp::Reverse(health.successes),
                health.failures,
                preferred_rank(endpoint),
            )
        });
        endpoints
    }

    fn available(&self, endpoint: &'static str) -> bool {
        self.endpoints
            .get(endpoint)
            .and_then(|health| health.cooldown_until)
            .is_none_or(|until| until <= Instant::now())
    }

    fn success(&mut self, endpoint: &'static str) {
        let health = self.endpoints.entry(endpoint).or_default();
        health.successes = health.successes.saturating_add(1);
        health.cooldown_until = None;
    }

    fn failure(&mut self, endpoint: &'static str, kind: FailureKind, policy: RetryPolicy) {
        let health = self.endpoints.entry(endpoint).or_default();
        health.failures = health.failures.saturating_add(1);
        let cooldown = match kind {
            FailureKind::RateLimited | FailureKind::GatewayTimeout | FailureKind::Server => {
                policy.overloaded_cooldown
            }
            _ => policy.transient_cooldown,
        };
        health.cooldown_until = Instant::now().checked_add(cooldown);
    }
}

pub(super) struct ReliableExecutor {
    transport: RequestTransport,
    preferred_order: Arc<Mutex<Vec<&'static str>>>,
    policy: RetryPolicy,
}

impl ReliableExecutor {
    pub(super) fn new(
        transport: RequestTransport,
        preferred_order: Arc<Mutex<Vec<&'static str>>>,
        policy: RetryPolicy,
    ) -> Self {
        Self {
            transport,
            preferred_order,
            policy,
        }
    }

    pub(super) fn query(
        &self,
        query: &str,
        deadline: Instant,
        health: &mut RunHealth,
        on_attempt: &dyn Fn(u32, u32, &'static str),
    ) -> Result<String, String> {
        let mut errors = Vec::new();
        let mut attempt = 0u32;
        let total = u32::from(self.policy.max_rounds) * OVERPASS_ENDPOINTS.len() as u32;
        for round in 0..self.policy.max_rounds {
            let preferred = self
                .preferred_order
                .lock()
                .expect("overpass endpoint order lock")
                .clone();
            let endpoints = health.ordered(&preferred);
            let mut attempted_this_round = false;
            for endpoint in endpoints {
                if Instant::now() >= deadline {
                    errors.push("总体截止已到，停止继续切换节点".to_owned());
                    return Err(errors.join("；"));
                }
                if !health.available(endpoint) {
                    continue;
                }
                attempted_this_round = true;
                attempt = attempt.saturating_add(1);
                on_attempt(attempt, total, endpoint);
                let timeout = self
                    .policy
                    .request_timeout
                    .min(deadline.saturating_duration_since(Instant::now()));
                let request = OverpassRequest {
                    endpoint,
                    body: format!("data={}", super::encode_query(query)),
                };
                let started = Instant::now();
                match (self.transport)(&request, timeout) {
                    Ok(body) => match validate_body(&body) {
                        Ok(()) => {
                            health.success(endpoint);
                            self.promote(endpoint);
                            log::info!(
                                "Overpass 节点 {endpoint} 成功，耗时 {:?}",
                                started.elapsed()
                            );
                            return Ok(body);
                        }
                        Err(failure) => {
                            health.failure(endpoint, failure.kind, self.policy);
                            errors.push(format_failure(endpoint, &failure));
                        }
                    },
                    Err(failure) => {
                        health.failure(endpoint, failure.kind, self.policy);
                        errors.push(format_failure(endpoint, &failure));
                    }
                }
            }
            if round + 1 < self.policy.max_rounds {
                wait(
                    self.policy
                        .retry_backoff
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            if !attempted_this_round && Instant::now() < deadline {
                errors.push("本次运行的可用节点均在冷却，停止继续请求".to_owned());
                break;
            }
        }
        if errors.is_empty() {
            Err("Overpass 节点列表为空或总体截止已到".to_owned())
        } else {
            Err(errors.join("；"))
        }
    }

    fn promote(&self, endpoint: &'static str) {
        if let Ok(mut order) = self.preferred_order.lock() {
            if let Some(position) = order.iter().position(|candidate| *candidate == endpoint) {
                let endpoint = order.remove(position);
                order.insert(0, endpoint);
            }
        }
    }
}

fn validate_body(body: &str) -> Result<(), RequestFailure> {
    let trimmed = body.trim_start();
    if trimmed.starts_with("<html")
        || body.contains("parse error")
        || body.contains("Runtime error")
        || body.contains("runtime error")
    {
        return Err(RequestFailure {
            kind: FailureKind::ErrorBody,
            message: super::first_error_line(body),
        });
    }
    let value: serde_json::Value = serde_json::from_str(body).map_err(|error| RequestFailure {
        kind: FailureKind::Parse,
        message: error.to_string(),
    })?;
    if !value
        .get("elements")
        .is_some_and(serde_json::Value::is_array)
    {
        return Err(RequestFailure {
            kind: FailureKind::Parse,
            message: "响应缺少 elements 数组".to_owned(),
        });
    }
    Ok(())
}

fn format_failure(endpoint: &str, failure: &RequestFailure) -> String {
    let kind = match failure.kind {
        FailureKind::Timeout => "连接超时",
        FailureKind::Connection => "连接失败",
        FailureKind::RateLimited => "HTTP 429 节点限流",
        FailureKind::GatewayTimeout => "HTTP 504 节点超时",
        FailureKind::Server => "服务端错误",
        FailureKind::ErrorBody => "服务端错误页",
        FailureKind::Parse => "响应解析失败",
    };
    format!("节点 {endpoint} {kind}: {}", failure.message)
}

fn wait(duration: Duration) {
    if duration.is_zero() {
        return;
    }
    let (_sender, receiver) = std::sync::mpsc::channel::<()>();
    let _ = receiver.recv_timeout(duration);
}

pub(super) fn production_transport(agent: ureq::Agent) -> RequestTransport {
    Arc::new(move |request: &OverpassRequest, timeout: Duration| {
        let url = format!("{}/api/interpreter", request.endpoint);
        let response = agent
            .post(&url)
            .timeout(timeout)
            .set("User-Agent", USER_AGENT)
            .set(
                "Content-Type",
                "application/x-www-form-urlencoded; charset=UTF-8",
            )
            .send_string(&request.body);
        match response {
            Ok(response) => response.into_string().map_err(|error| RequestFailure {
                kind: FailureKind::Parse,
                message: error.to_string(),
            }),
            Err(ureq::Error::Status(status, response)) => Err(RequestFailure {
                kind: match status {
                    429 => FailureKind::RateLimited,
                    504 => FailureKind::GatewayTimeout,
                    500..=599 => FailureKind::Server,
                    _ => FailureKind::Connection,
                },
                message: response.status_text().to_owned(),
            }),
            Err(ureq::Error::Transport(error)) => {
                let message = error.to_string();
                let lower = message.to_ascii_lowercase();
                Err(RequestFailure {
                    kind: if lower.contains("timed out") || lower.contains("timeout") {
                        FailureKind::Timeout
                    } else {
                        FailureKind::Connection
                    },
                    message,
                })
            }
        }
    })
}
