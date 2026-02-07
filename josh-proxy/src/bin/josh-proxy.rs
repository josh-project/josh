use anyhow::{Context, anyhow};
use josh_core::cache::CacheStack;
use josh_proxy::service::{JoshProxyService, make_service};
use josh_proxy::upstream::{RemoteAuth, RepoUpdate};
use josh_proxy::{FetchError, TmpGitNamespace};

use clap::Parser;
use tokio::sync::broadcast;
use tracing_futures::Instrument;

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

async fn shutdown_signal(shutdown_tx: broadcast::Sender<()>) {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
    let _ = shutdown_tx.send(());
    println!("shutdown_signal");
}

fn init_trace() -> Option<opentelemetry_sdk::trace::SdkTracerProvider> {
    use opentelemetry::{KeyValue, global, trace::TracerProvider};
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
    use tracing_subscriber::Layer;

    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::propagation::TraceContextPropagator;

    // Set format for propagating tracing context. This allows to link traces from one invocation
    // of josh to the next
    global::set_text_map_propagator(TraceContextPropagator::new());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(false)
        .with_writer(io::stderr);

    let filter = match std::env::var("RUST_LOG") {
        Ok(_) => tracing_subscriber::EnvFilter::from_default_env(),
        _ => tracing_subscriber::EnvFilter::new("josh=trace,josh_proxy=trace"),
    };

    let service_name = std::env::var("JOSH_SERVICE_NAME").unwrap_or("josh-proxy".to_owned());

    if let Ok(endpoint) =
        std::env::var("JOSH_OTLP_ENDPOINT").or(std::env::var("JOSH_JAEGER_ENDPOINT"))
    {
        let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .expect("failed to build OTLP endpoint");

        let resource = opentelemetry_sdk::Resource::builder()
            .with_attribute(KeyValue::new(SERVICE_NAME, service_name.clone()))
            .build();

        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(otlp_exporter)
            .build();

        let tracer = tracer_provider.tracer(service_name);
        global::set_tracer_provider(tracer_provider.clone());

        let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = filter
            .and_then(fmt_layer)
            .and_then(telemetry_layer)
            .with_subscriber(tracing_subscriber::Registry::default());

        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");

        Some(tracer_provider)
    } else {
        let subscriber = filter
            .and_then(fmt_layer)
            .with_subscriber(tracing_subscriber::Registry::default());
        tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");

        None
    }
}

async fn run_polling(serv: Arc<JoshProxyService>) -> anyhow::Result<()> {
    loop {
        let polls = serv.poll.lock().unwrap().clone();

        for (upstream_repo, auth, url) in polls {
            let remote_auth = RemoteAuth::Http { auth };
            let fetch_result = josh_proxy::upstream::fetch_upstream(
                serv.clone(),
                &upstream_repo,
                &remote_auth,
                url.clone(),
                None,
                None,
                true,
            )
            .in_current_span()
            .await;

            match fetch_result {
                Ok(()) => {}
                Err(FetchError::Other(e)) => return Err(e),
                Err(FetchError::AuthRequired) => {
                    return Err(anyhow!("auth: access denied while polling"));
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}

async fn run_housekeeping(local: std::path::PathBuf, gc: bool) -> anyhow::Result<()> {
    let mut i: usize = 0;
    let cache = std::sync::Arc::new(CacheStack::default());

    loop {
        let local = local.clone();
        let cache = cache.clone();

        tokio::task::spawn_blocking(move || {
            let do_gc = (i % 60 == 0) && gc;
            josh_proxy::housekeeping::run(&local, cache, do_gc)
        })
        .await??;
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        i += 1;
    }
}

fn io_thread(mut rx: tokio::sync::mpsc::UnboundedReceiver<josh_proxy::service::IoCleanup>) {
    use josh_proxy::service::IoCleanup;

    while let Some(IoCleanup { repo_path, name }) = rx.blocking_recv() {
        TmpGitNamespace::cleanup(&repo_path, &name);
    }
}

async fn run_proxy(args: josh_proxy::cli::Args) -> anyhow::Result<i32> {
    let local = std::path::PathBuf::from(&args.local.as_ref().unwrap());
    let local = if local.is_absolute() {
        local
    } else {
        std::env::current_dir()?.join(local)
    };

    let (io_thread_tx, io_thread_rx) = tokio::sync::mpsc::unbounded_channel();
    let io_thread = tokio::task::spawn_blocking(move || io_thread(io_thread_rx));

    josh_proxy::service::create_repo(&local, None)?;
    josh_core::cache::sled_load(&local)?;

    let proxy_service = make_service()
        .port(args.port)
        .repo_path(&local)
        .remotes(&args.remote)
        .require_auth(args.require_auth)
        .cache_duration(args.cache_duration)
        .io_thread_tx(io_thread_tx)
        .maybe_filter_prefix(args.filter_prefix)
        .maybe_poll_user(args.poll_user)
        .call()?;

    let ps = proxy_service.clone();

    // Create axum router
    let app = josh_proxy::service::make_service_router(proxy_service);

    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    let addr: SocketAddr = format!("[::]:{}", args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    let server_future = async move { axum::serve(listener, app).await.context("Server error") };

    eprintln!("Now listening on {}", addr);

    if args.no_background {
        tokio::select!(
            r = server_future => eprintln!("http server exited: {:?}", r),
            _ = shutdown_signal(shutdown_tx) => eprintln!("shutdown requested"),
        );
    } else {
        tokio::select!(
            r = run_housekeeping(local, args.gc) => eprintln!("run_housekeeping exited: {:?}", r),
            r = run_polling(ps) => eprintln!("run_polling exited: {:?}", r),
            r = server_future => eprintln!("http server exited: {:?}", r),
            _ = shutdown_signal(shutdown_tx) => eprintln!("shutdown requested"),
        );
    }

    // Once sender is dropped, IO thread will finish
    io_thread.await?;

    Ok(0)
}

fn repo_update_from_env() -> anyhow::Result<crate::RepoUpdate> {
    let repo_update = std::env::var("JOSH_REPO_UPDATE").context("JOSH_REPO_UPDATE not set")?;

    serde_json::from_str(&repo_update).context("Failed to parse JOSH_REPO_UPDATE")
}

fn update_hook(refname: &str, old: &str, new: &str) -> anyhow::Result<i32> {
    let mut repo_update = repo_update_from_env()?;

    repo_update
        .refs
        .insert(refname.to_owned(), (old.to_owned(), new.to_owned()));

    let client = reqwest::blocking::Client::builder().timeout(None).build()?;
    let resp = client
        .post(format!("http://localhost:{}/repo_update", repo_update.port))
        .json(&repo_update)
        .send();

    match resp {
        Ok(resp) => {
            let success = resp.status().is_success();
            println!("upstream: response status: {}", resp.status());

            match resp.text() {
                Ok(text) if text.trim().is_empty() => {
                    println!("upstream: no response body");
                }
                Ok(text) => {
                    println!("upstream: response body:\n\n{}", text);
                }
                Err(err) => {
                    println!("upstream: warn: failed to read response body: {:?}", err);
                }
            }

            if success { Ok(0) } else { Ok(1) }
        }
        Err(err) => {
            tracing::warn!("/repo_update request failed {:?}", err);
            Ok(1)
        }
    }
}

fn pre_receive_hook() -> anyhow::Result<i32> {
    let repo_update = repo_update_from_env()?;

    let push_options_path = std::path::PathBuf::from(repo_update.git_dir)
        .join("refs/namespaces")
        .join(repo_update.git_ns)
        .join("push_options");

    let push_option_count: usize = std::env::var("GIT_PUSH_OPTION_COUNT")?.parse()?;

    let mut push_options = HashMap::<String, serde_json::Value>::new();
    for i in 0..push_option_count {
        let push_option = std::env::var(format!("GIT_PUSH_OPTION_{}", i))?;
        if let Some((key, value)) = push_option.split_once("=") {
            push_options.insert(key.into(), value.into());
        } else {
            push_options.insert(push_option, true.into());
        }
    }

    std::fs::write(push_options_path, serde_json::to_string(&push_options)?)?;

    Ok(0)
}

fn main() {
    // josh-proxy creates a symlink to itself as a git update hook.
    // When it gets called by git as that hook, the binary name will end
    // end in "/update" and this will not be a new server.
    // The update hook will then make a http request back to the main
    // process to do the actual computation while taking advantage of the
    // cached data already loaded into the main process's memory.
    if let [a0, a1, a2, a3, ..] = &std::env::args().collect::<Vec<_>>().as_slice()
        && a0.ends_with("/update")
    {
        std::process::exit(update_hook(a1, a2, a3).unwrap_or(1));
    }

    if let [a0, ..] = &std::env::args().collect::<Vec<_>>().as_slice()
        && a0.ends_with("/pre-receive")
    {
        eprintln!("josh-proxy: pre-receive hook");
        let code = match pre_receive_hook() {
            Ok(code) => code,
            Err(e) => {
                eprintln!("josh-proxy: pre-receive hook failed: {}", e);
                std::process::exit(1);
            }
        };

        std::process::exit(code);
    }

    let args = josh_proxy::cli::Args::parse();
    let exit_code = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let tracer_provider = init_trace();
            let exit_code = run_proxy(args).await.unwrap_or(1);

            if let Some(tracer_provider) = tracer_provider {
                tracer_provider
                    .shutdown()
                    .expect("failed to shutdown tracer");
            }

            exit_code
        });

    std::process::exit(exit_code);
}
