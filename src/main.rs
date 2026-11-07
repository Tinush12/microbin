use std::io::Write;
use std::path::PathBuf;

use actix_web::middleware::Condition;
use actix_web::{middleware, web, App, HttpServer};
use actix_web_httpauth::middleware::HttpAuthentication;
use chrono::Local;
use env_logger::Builder;
use log::LevelFilter;

use crate::args::ARGS;
use crate::endpoints::static_resources;
use crate::feed::{pages, FeedConfig, FeedState};

pub mod args;
pub mod feed;

pub mod util {
    pub mod auth;
}

pub mod endpoints {
    pub mod static_resources;
}

const BYTES_PER_MB: u64 = 1024 * 1024;

fn basic_auth_enabled() -> bool {
    ARGS.auth_basic_username
        .as_ref()
        .is_some_and(|username| !username.trim().is_empty())
}

fn feed_state() -> web::Data<FeedState> {
    let config = FeedConfig {
        data_dir: PathBuf::from(&ARGS.data_dir),
        max_file_bytes: ARGS.max_file_size_mb.saturating_mul(BYTES_PER_MB),
        title: ARGS.title.clone(),
    };
    match FeedState::open(config) {
        Ok(state) => web::Data::new(state),
        Err(error) => panic!("Couldn't open the feed in {}: {error}", ARGS.data_dir),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .filter(None, LevelFilter::Info)
        .init();

    log::info!("MicroBin starting on http://{}:{}", ARGS.bind, ARGS.port);

    let state = feed_state();

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(middleware::NormalizePath::trim())
            .wrap(
                middleware::Logger::new(r#"%{r}a "%r" %s %b "%{Referer}i" "%{User-Agent}i" %T"#)
            // `%{r}a` is actix's built‑in "real ip" token, which uses
            // ConnectionInfo::realip_remote_addr(). it picks up headers like
            // X-Real-IP / X-Forwarded-For when the framework is behind a proxy.
            )
            .service(static_resources::static_resources)
            .service(
                web::scope("")
                    .wrap(Condition::new(
                        basic_auth_enabled(),
                        HttpAuthentication::basic(util::auth::auth_validator),
                    ))
                    .configure(feed::configure),
            )
            .default_service(web::route().to(pages::fallback))
    })
    .bind((ARGS.bind, ARGS.port))?
    .workers(ARGS.threads as usize)
    .run()
    .await
}
