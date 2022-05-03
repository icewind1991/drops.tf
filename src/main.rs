use crate::data::{DataSource, DropStats, GlobalStats, SearchParams, TopOrder, TopStats};
use askama::Template;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use main_error::MainError;
use sqlx::postgres::PgPool;
use std::convert::TryInto;
use std::error::Error;
use std::fmt::{self, Debug, Display};
use std::net::SocketAddr;
use std::str::FromStr;
use tower_http::trace::TraceLayer;
use tracing::instrument;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod data;

pub struct DropsError(Box<dyn Error + Send + Sync + 'static>);

impl<E: Into<Box<dyn Error + Send + Sync + 'static>>> From<E> for DropsError {
    fn from(e: E) -> Self {
        DropsError(e.into())
    }
}

impl Debug for DropsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)?;
        let mut source = self.0.source();
        while let Some(error) = source {
            write!(f, "\ncaused by: {}", error)?;
            source = error.source();
        }
        Ok(())
    }
}

impl IntoResponse for DropsError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("{:?}", self)).into_response()
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    top: &'a [TopStats],
    stats: GlobalStats,
}

#[derive(Template)]
#[template(path = "player.html")]
struct PlayerTemplate {
    stats: DropStats,
}

#[derive(Template)]
#[template(path = "notfound.html")]
struct NotFoundTemplate;

#[derive(Template)]
#[template(path = "404.html")]
struct PageNotFoundTemplate;

#[instrument(skip(data_source))]
async fn page_top_stats(
    Extension(data_source): Extension<DataSource>,
    order: TopOrder,
) -> Result<impl IntoResponse, DropsError> {
    let top = data_source.top_stats(order).await?;
    let stats = data_source.global_stats().await?;
    let template = IndexTemplate {
        top: top.as_slice(),
        stats,
    };

    Ok(Html(template.render().unwrap()))
}

#[instrument(skip(data_source))]
async fn page_player(
    Extension(data_source): Extension<DataSource>,
    Path(steam_id): Path<String>,
) -> Result<impl IntoResponse, DropsError> {
    let steam_id = match steam_id.as_str().try_into().map_err(DropsError::from) {
        Ok(steam_id) => steam_id,
        Err(e) => data_source.resolve_vanity_url(&steam_id).await?.ok_or(e)?,
    };
    let stats = match data_source.stats_for_user(steam_id).await {
        Ok(stats) => stats,
        Err(_) => {
            let template = NotFoundTemplate;
            return Ok(Html(template.render().unwrap()));
        }
    };
    let template = PlayerTemplate { stats };
    Ok(Html(template.render().unwrap()))
}

#[instrument(skip(data_source))]
pub async fn api_search(
    Extension(data_source): Extension<DataSource>,
    Query(query): Query<SearchParams>,
) -> Result<impl IntoResponse, DropsError> {
    let result = data_source.player_search(&query.search).await?;
    Ok(Json(result))
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    if let Ok(tracing_endpoint) = dotenv::var("TRACING_ENDPOINT") {
        let tracer = opentelemetry_jaeger::new_pipeline()
            .with_agent_endpoint(tracing_endpoint)
            .with_service_name("drops.tf")
            .install_simple()?;
        let open_telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        tracing_subscriber::registry()
            .with(open_telemetry)
            .try_init()?;
    }

    let database_url = dotenv::var("DATABASE_URL")?;
    let api_key = dotenv::var("STEAM_API_KEY")?;
    let port = u16::from_str(&dotenv::var("PORT")?)?;

    let pool = PgPool::connect(&database_url).await?;
    let data_source = DataSource::new(pool, api_key);

    let app = Router::new()
        .route(
            "/",
            get(|data_source| page_top_stats(data_source, TopOrder::Drops)),
        )
        .route(
            "/dpg",
            get(|data_source| page_top_stats(data_source, TopOrder::Dpg)),
        )
        .route(
            "/dph",
            get(|data_source| page_top_stats(data_source, TopOrder::Dps)),
        )
        .route(
            "/dpu",
            get(|data_source| page_top_stats(data_source, TopOrder::Dpu)),
        )
        .route("/profile/:steam_id", get(page_player))
        .route("/search", get(api_search))
        .layer(Extension(data_source))
        .layer(TraceLayer::new_for_http());

    // let not_found = warp::any().map(|| {
    //     return Ok(warp::reply::html(PageNotFoundTemplate.render().unwrap()));
    // });

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}
