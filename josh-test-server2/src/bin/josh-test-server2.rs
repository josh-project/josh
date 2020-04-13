use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Request, Response, Server};
use std::env;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use hyper::header::{AUTHORIZATION};

/* fn auth_response( */
/*     req: &Request<Body>, */
/*     username: &str, */
/*     password: &str, */
/* ) -> Option<Response<Body>> { */
/*     let (rusername, rpassword) = match req.headers().get() { */
/*         Some(&Authorization(Basic { */
/*             ref username, */
/*             ref password, */
/*         })) => ( */
/*             username.to_owned(), */
/*             password.to_owned().unwrap_or_else(|| "".to_owned()), */
/*         ), */
/*         _ => { */
/*             println!("ServeTestGit: no credentials in request"); */
/*             let mut response = */
/*                 Response::new().with_status(hyper::StatusCode::Unauthorized); */
/*             response.headers_mut().set_raw( */
/*                 "WWW-Authenticate", */
/*                 "Basic realm=\"User Visible Realm\"", */
/*             ); */
/*             return Some(response); */
/*         } */
/*     }; */

/*     if rusername != "admin" && (rusername != username || rpassword != password) */
/*     { */
/*         println!("ServeTestGit: wrong user/pass"); */
/*         println!("user: {:?} - {:?}", rusername, username); */
/*         println!("pass: {:?} - {:?}", rpassword, password); */
/*         let mut response = */
/*             Response::new().with_status(hyper::StatusCode::Unauthorized); */
/*         response */
/*             .headers_mut() */
/*             .set_raw("WWW-Authenticate", "Basic realm=\"User Visible Realm\""); */
/*         return Some(response); */
/*     } */

/*     println!("CREDENTIALS OK {:?} {:?}", &rusername, &rpassword); */
/*     return None; */
/* } */

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


    let make_service = make_service_fn(move |_| {
        let myservice = myservice.clone();

        async move {
            Ok::<_, Error>(service_fn(move |_req| {
                let myservice = myservice.clone();
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
