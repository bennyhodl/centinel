//! `/workspace/rolodex` — the Rolodex page: the people the corpus names, and one person.
//!
//! | Route | Purpose |
//! |---|---|
//! | `GET /workspace/rolodex?name=&source=` | the names, most-named first; see [`centinel_core::rolodex`] |
//! | `GET /workspace/rolodex/person?name=&source=` | one name: its addresses, accounts, pages, and every source it comes up in |
//!
//! Both answer through the `rolodex` op, so the page, the CLI and MCP read one report. Any
//! approved user may read them, as they may read Search and the Graph.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use centinel_core::op::Ctx;
use centinel_core::ops::{self, PersonArgs, RolodexAction, RolodexArgs};
use serde::Deserialize;

use super::workspace_error;

/// Names a list returns before it says how many more there are. A page is for finding one
/// person, and a narrower search is how to reach the rest.
const LISTED: usize = 200;

pub fn routes() -> axum::Router<Arc<Ctx>> {
    use axum::routing::get;
    axum::Router::new()
        .route("/workspace/rolodex", get(list))
        .route("/workspace/rolodex/person", get(person))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize)]
struct PersonQuery {
    name: String,
    #[serde(default)]
    source: Option<String>,
}

/// An empty parameter is no filter: a cleared search box sends `name=`.
fn given(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

async fn list(State(ctx): State<Arc<Ctx>>, Query(query): Query<ListQuery>) -> Response {
    answer(
        &ctx,
        RolodexArgs {
            action: None,
            source: given(query.source),
            name: given(query.name),
            max: LISTED,
        },
    )
    .await
}

async fn person(State(ctx): State<Arc<Ctx>>, Query(query): Query<PersonQuery>) -> Response {
    answer(
        &ctx,
        RolodexArgs {
            action: Some(RolodexAction::Show(PersonArgs { name: query.name })),
            source: given(query.source),
            ..Default::default()
        },
    )
    .await
}

async fn answer(ctx: &Ctx, args: RolodexArgs) -> Response {
    match ops::rolodex(ctx, args).await {
        Ok(report) => Json(report).into_response(),
        Err(error) => workspace_error(error),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use centinel_core::config::SourceConfig;
    use centinel_core::content::ContentKind;
    use centinel_core::crumbs::Ledger;
    use centinel_core::domain::{Resource, SourceId};
    use centinel_core::op::Ctx;
    use centinel_core::store::Store;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use tower::ServiceExt;

    async fn get(app: axum::Router, uri: &str) -> (StatusCode, Value) {
        let response = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, serde_json::from_slice(&body).unwrap())
    }

    /// The page's two reads, over a ledger row as `collect` leaves it: a list that finds the
    /// name by a word of it, and the person behind it with the source that named them.
    #[tokio::test]
    async fn the_page_lists_a_name_and_opens_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).await.unwrap();
        centinel_core::catalog::Sources::new(&store)
            .add(&SourceConfig::site("tampa", "https://www.tampa.gov"))
            .await
            .unwrap();
        // As `collect` leaves a page: an Observation in the log, then its ledger row.
        let html = r#"<a href="mailto:lviera@tampagov.net">Luis Viera</a>"#;
        let resource = Resource::new(
            SourceId::new("tampa").unwrap(),
            "https://www.tampa.gov/council",
        );
        let obs = store
            .record_observation(
                &resource,
                html.as_bytes(),
                "2026-10-01T00:00:00Z".parse().unwrap(),
                BTreeMap::from([("content-type".to_string(), "text/html".to_string())]),
            )
            .await
            .unwrap();
        Ledger::new(&store, resource.source.clone())
            .record(&resource, html.as_bytes(), ContentKind::Html, &obs)
            .await;
        let app = crate::http::router(Arc::new(Ctx::new(store)));

        let (status, list) = get(app.clone(), "/workspace/rolodex?name=viera&source=").await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["people"][0]["name"], "Luis Viera");

        let (status, person) = get(app, "/workspace/rolodex/person?name=Luis%20Viera").await;
        assert_eq!(status, StatusCode::OK, "{person}");
        assert_eq!(person["person"]["emails"]["lviera@tampagov.net"], 1);
        assert_eq!(person["sources"][0]["source"], "tampa");
        assert_eq!(person["sources"][0]["linked"], 1);
    }
}
