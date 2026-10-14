//! `/workspace/graph` — the Graph page: sources, the hosts they link to, and how much.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /workspace/graph?min_pages=&hide_refused=` | nodes, edges and the whole graph's totals; see [`centinel_core::graph`] |
//! | `GET /workspace/graph/host/{host}` | one host's card: who links there, where on it, from which pages |
//! | `POST /workspace/graph/host/{host}/refuse` | `{ "note"? }`: the ruling `crumbs ignore` writes |
//! | `POST /workspace/graph/host/{host}/allow` | take it back |
//!
//! Approved browser users may read the graph and host cards. Mutations require an
//! administrator's browser session. A refusal is the same corpus-wide ruling the CLI and the
//! Sources page write — one `decisions.jsonl`, one op — so the three can never disagree
//! about what the corpus will not look at.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use centinel_core::crumbs::Ruling;
use centinel_core::graph::{Filter, Graph, HostDetail};
use centinel_core::op::{Cancel, Ctx, Progress};
use centinel_core::ops::{self, CrumbsAction, CrumbsArgs, RuleArgs};
use serde::Deserialize;

use super::workspace_error;

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/workspace/graph", get(graph))
        .route("/workspace/graph/host/{host}", get(host))
        .route("/workspace/graph/host/{host}/refuse", post(refuse))
        .route("/workspace/graph/host/{host}/allow", post(allow))
}

async fn graph(State(ctx): State<Arc<Ctx>>, Query(filter): Query<Filter>) -> Response {
    match Graph::read(&ctx.store).await {
        Ok(graph) => Json(graph.filtered(&filter)).into_response(),
        Err(error) => workspace_error(error),
    }
}

async fn host(State(ctx): State<Arc<Ctx>>, Path(host): Path<String>) -> Response {
    match HostDetail::read(&ctx.store, &host).await {
        Ok(detail) => Json(detail).into_response(),
        Err(error) => workspace_error(error),
    }
}

/// What a refusal may carry: why, read back on the host's card.
#[derive(Default, Deserialize)]
struct RuleBody {
    #[serde(default)]
    note: Option<String>,
}

async fn refuse(
    State(ctx): State<Arc<Ctx>>,
    Path(host): Path<String>,
    body: Option<Json<RuleBody>>,
) -> Response {
    rule(
        &ctx,
        host,
        Ruling::Ignore,
        body.map(|Json(b)| b).unwrap_or_default(),
    )
    .await
}

async fn allow(
    State(ctx): State<Arc<Ctx>>,
    Path(host): Path<String>,
    body: Option<Json<RuleBody>>,
) -> Response {
    rule(
        &ctx,
        host,
        Ruling::Allow,
        body.map(|Json(b)| b).unwrap_or_default(),
    )
    .await
}

/// Through the `crumbs` op, which owns the ruling's spelling and the host's.
async fn rule(ctx: &Ctx, host: String, ruling: Ruling, body: RuleBody) -> Response {
    let args = RuleArgs {
        host,
        note: body.note,
    };
    let action = match ruling {
        Ruling::Ignore => CrumbsAction::Ignore(args),
        Ruling::Allow => CrumbsAction::Allow(args),
    };
    let args = CrumbsArgs {
        action: Some(action),
        ..Default::default()
    };
    match ops::crumbs(ctx, args, &Progress::none(), &Cancel::none()).await {
        Ok(report) => Json(report).into_response(),
        Err(error) => workspace_error(error),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use centinel_core::config::SourceConfig;
    use centinel_core::crumbs::{Carrier, Ledger, scan_page};
    use centinel_core::domain::SourceId;
    use centinel_core::store::Store;
    use serde_json::Value;
    use tower::ServiceExt;

    use super::*;

    /// A store with one source whose one page links two hosts, behind the admin router.
    async fn app() -> (tempfile::TempDir, axum::Router) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        centinel_core::catalog::Sources::new(&store)
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();
        let row = scan_page(
            r#"<a href="https://www.facebook.com/CityofTampa">social</a>
               <a href="https://publicrec.hillsclerk.com/Civil/">records</a>"#,
            Carrier::at_address("https://www.tampa.gov/clerk"),
        );
        Ledger::new(&store, SourceId::new("tampa").unwrap())
            .append(&row)
            .await
            .unwrap();
        let app = crate::http::router(Arc::new(Ctx::new(store)))
            .layer(axum::Extension(crate::access::Role::Admin));
        (dir, app)
    }

    async fn json(resp: Response) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn get(app: &axum::Router, path: &str) -> Value {
        let resp = app
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{path}");
        json(resp).await
    }

    fn post(path: &str, origin: &str) -> Request<Body> {
        Request::post(path)
            .header("host", "127.0.0.1:8787")
            .header("origin", origin)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"note":"a social network"}"#))
            .unwrap()
    }

    fn host_standings(graph: &Value) -> Vec<(String, String)> {
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"] == "host")
            .map(|n| {
                (
                    n["host"].as_str().unwrap().to_string(),
                    n["standing"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    /// Refuse, see it marked and then hidden, allow, see it back — the round trip the
    /// drawer's button makes, through the same ruling the CLI writes.
    #[tokio::test]
    async fn refusing_a_host_round_trips_through_the_graph() {
        let (_d, app) = app().await;
        let whole = get(&app, "/workspace/graph").await;
        assert_eq!(whole["totals"]["hosts"], 2);
        assert_eq!(whole["totals"]["open"], 2);
        assert_eq!(whole["edges"].as_array().unwrap().len(), 2);

        let resp = app
            .clone()
            .oneshot(post(
                "/workspace/graph/host/www.facebook.com/refuse",
                "http://127.0.0.1:8787",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ruled = json(resp).await;
        assert_eq!(ruled["ruling"], "ignore");
        assert_eq!(ruled["note"], "a social network");

        let marked = get(&app, "/workspace/graph").await;
        assert!(host_standings(&marked).contains(&("www.facebook.com".into(), "ignored".into())));
        assert_eq!(marked["totals"]["refused"], 1);

        let hidden = get(&app, "/workspace/graph?hide_refused=true&min_pages=1").await;
        assert_eq!(
            host_standings(&hidden),
            [("publicrec.hillsclerk.com".to_string(), "open".to_string())]
        );

        let card = get(&app, "/workspace/graph/host/www.facebook.com").await;
        assert_eq!(card["standing"], "ignored");
        assert_eq!(card["note"], "a social network");
        assert_eq!(card["sources"][0]["source"], "tampa");
        assert_eq!(card["paths"][0]["path"], "/CityofTampa");

        let resp = app
            .clone()
            .oneshot(post(
                "/workspace/graph/host/www.facebook.com/allow",
                "http://127.0.0.1:8787",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let back = get(&app, "/workspace/graph?hide_refused=true").await;
        assert_eq!(host_standings(&back).len(), 2);
    }

    /// A ruling is a write, so it needs the browser's own origin.
    #[tokio::test]
    async fn a_ruling_from_another_origin_is_refused() {
        let (_d, app) = app().await;
        let resp = app
            .clone()
            .oneshot(post(
                "/workspace/graph/host/www.facebook.com/refuse",
                "http://attacker.example",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert_eq!(get(&app, "/workspace/graph").await["totals"]["refused"], 0);
    }

    /// The filter is the query string, and a filter that leaves nothing still answers
    /// with the sources and the whole graph's totals.
    #[tokio::test]
    async fn min_pages_filters_edges_and_keeps_the_totals() {
        let (_d, app) = app().await;
        let thin = get(&app, "/workspace/graph?min_pages=5").await;
        assert_eq!(thin["edges"].as_array().unwrap().len(), 0);
        assert_eq!(thin["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(thin["nodes"][0]["kind"], "source");
        assert_eq!(thin["totals"]["edges"], 2);
        assert_eq!(thin["truncated"], false);
    }
}
