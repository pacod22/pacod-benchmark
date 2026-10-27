//! Who calls the API, and what they may do.
//!
//! The UI's server side sends the signed-in person's access token (`aud` pacod-bench) as a
//! bearer token. It is checked against pacod-id's keys, and the person's benchmark roles come
//! from the copy of the bindings pacod-id's feed keeps current. With `BENCH_AUTH=dev` one
//! static token stands for an admin instead, for development without pacod-id.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use pacod_id_authz::{Binding, ScopeRef, Subject};
use pacod_id_client::{AuthError, Caller};
use subtle::ConstantTimeEq;

use super::{ApiError, AppState};
use crate::identity::{CATALOG, Identity};

pub enum AuthMode {
    Oidc(Arc<Identity>),
    Dev { token: String },
}

/// The permissions of the manifest, in the order the UI shows them.
pub const PERMISSIONS: [&str; 6] = [
    "bench.results.read",
    "bench.suites.manage",
    "bench.runs.start",
    "bench.reports.publish",
    "bench.targets.manage",
    "bench.members.manage",
];

#[derive(Clone)]
pub struct Principal {
    pub id: String,
    /// Email or name, for the run records and the UI.
    pub display: String,
    pub kind: &'static str,
    subject: Subject,
}

impl Principal {
    pub fn new(id: String, display: String, kind: &'static str, bindings: Vec<Binding>) -> Principal {
        let subject = if kind == "service" { Subject::service(bindings) } else { Subject::user(bindings) };
        Principal { id, display, kind, subject }
    }

    pub fn allowed(&self, permission: &str) -> bool {
        CATALOG.allowed(&self.subject, permission, &[ScopeRef::platform()])
    }

    pub fn require(&self, permission: &str) -> Result<(), ApiError> {
        if self.allowed(permission) {
            Ok(())
        } else {
            Err(ApiError::new(StatusCode::FORBIDDEN, "forbidden", format!("this needs {permission}")))
        }
    }

    pub fn permissions(&self) -> Vec<&'static str> {
        PERMISSIONS.into_iter().filter(|p| self.allowed(p)).collect()
    }
}

fn bearer(parts: &Parts) -> Option<&str> {
    let value = parts.headers.get("authorization")?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))?.trim();
    (!token.is_empty()).then_some(token)
}

fn unauthorized() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "invalid or expired credentials")
}

impl FromRequestParts<AppState> for Principal {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).ok_or_else(unauthorized)?;
        let p = match state.auth.as_ref() {
            AuthMode::Dev { token: dev } => {
                if !bool::from(token.as_bytes().ct_eq(dev.as_bytes())) {
                    return Err(unauthorized());
                }
                Principal::new(
                    "dev".into(),
                    "development".into(),
                    "user",
                    vec![Binding::new("bench.admin", vec![ScopeRef::platform()])],
                )
            }
            AuthMode::Oidc(id) => {
                if !id.fresh() {
                    return Err(ApiError::unavailable(
                        "pacod-id's copy of the roles is not current; try again shortly",
                    ));
                }
                let caller = id.auth.authenticate(token).await.map_err(|e| match e {
                    AuthError::Unavailable(why) => {
                        tracing::warn!(error = %why, "pacod-id could not check a token");
                        ApiError::unavailable("pacod-id cannot be reached; try again shortly")
                    }
                    _ => unauthorized(),
                })?;
                match caller {
                    Caller::User { sub, email, name, bindings, .. } => {
                        let display = email.or(name).unwrap_or_else(|| sub.clone());
                        Principal::new(sub, display, "user", bindings)
                    }
                    Caller::Service { sub, name, bindings, .. } => Principal::new(sub, name, "service", bindings),
                }
            }
        };
        // Everything but the health checks needs some benchmark role.
        p.require("bench.results.read")
            .map_err(|_| ApiError::new(StatusCode::FORBIDDEN, "no_access", "this account has no benchmark role"))?;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_role_gets_its_permissions_and_nothing_else() {
        let p = |role: &str| {
            Principal::new("u".into(), "u".into(), "user", vec![Binding::new(role, vec![ScopeRef::platform()])])
        };
        assert_eq!(p("bench.viewer").permissions(), ["bench.results.read"]);
        assert_eq!(
            p("bench.operator").permissions(),
            ["bench.results.read", "bench.suites.manage", "bench.runs.start", "bench.reports.publish"]
        );
        assert_eq!(p("bench.admin").permissions().len(), 6);
        assert!(p("gw.platform_admin").permissions().is_empty(), "another product's role grants nothing here");
    }
}
