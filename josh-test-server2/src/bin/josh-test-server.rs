use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response, Server};
use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

struct MyService {
    num: usize,
}

#[tokio::main]
async fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };
    let app = clap::App::new("josh-test-server")
        .arg(
            clap::Arg::with_name("local")
                .long("local")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("port").long("port").takes_value(true))
        .arg(
            clap::Arg::with_name("password")
                .long("password")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("username")
                .long("username")
                .takes_value(true),
        );

    let args = app.get_matches_from(args);

    let port = args.value_of("port").unwrap_or("8000").to_owned();
    let addr = format!("0.0.0.0:{}", port).parse().unwrap();

    let myservice = Arc::new(MyService { num: 0 });


    // The closure inside `make_service_fn` is run for each connection,
    // creating a 'service' to handle requests for that specific connection.
    let make_service = make_service_fn(move |_| {
        // While the state was moved into the make_service closure,
        // we need to clone it here because this closure is called
        // once for every connection.
        //
        // Each connection could send multiple requests, so
        // the `Service` needs a clone to handle later requests.
        let myservice = myservice.clone();

        async move {
            // This is the `Service` that will handle the connection.
            // `service_fn` is a helper to convert a function that
            let myservice = myservice.clone();
            // returns a Response into a `Service`.
            Ok::<_, Error>(service_fn(move |_req| {
                // Get the current count, and also increment by 1, in a single
                let myservice = myservice.clone();
                // atomic operation.
                async move {
                    Ok::<_, Error>(Response::new(Body::from(format!(
                        "Request #{}",
                        myservice.num
                    ))))
                }
            }))
        }
    });

    // Then bind and serve...
    let server = Server::bind(&addr).serve(make_service);

    // And run forever...
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
