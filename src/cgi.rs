extern crate futures;
extern crate hyper;
/* extern crate tokio; */
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_process;

use self::futures::future::Future;
use self::futures::Stream;
use self::hyper::header::ContentEncoding;
use self::hyper::header::ContentLength;
use self::hyper::header::ContentType;
use self::hyper::server::{Request, Response};
use cgi::tokio_io::AsyncWrite;
use cgi::tokio_process::CommandExt;
/* use tokio::prelude::*; */
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;

pub fn do_cgi(
    req: Request,
    cmd: Command,
    handle: tokio_core::reactor::Handle,
) -> Box<Future<Item = Response, Error = hyper::Error>> {
    trace_scoped!("do_cgi");
    let mut cmd = cmd;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    cmd.stdin(Stdio::piped());
    println!("REQUEST_METHOD {:?}", req.method());
    cmd.env("SERVER_SOFTWARE", "hyper")
        .env("SERVER_NAME", "localhost") // TODO
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", "HTTP/1.1") // TODO
        .env("SERVER_PORT", "80") // TODO
        .env("REQUEST_METHOD", format!("{}", req.method()))
        .env("SCRIPT_NAME", "") // TODO
        .env("QUERY_STRING", req.query().unwrap_or(""))
        .env("REMOTE_ADDR", "") // TODO
        .env("AUTH_TYPE", "") // TODO
        .env("REMOTE_USER", "") // TODO
        .env(
            "CONTENT_TYPE",
            &format!(
                "{}",
                req.headers().get().unwrap_or(&ContentType::plaintext())
            ),
        )
        .env(
            "HTTP_CONTENT_ENCODING",
            &format!(
                "{}",
                req.headers().get().unwrap_or(&ContentEncoding(vec![]))
            ),
        )
        .env(
            "CONTENT_LENGTH",
            &format!("{}", req.headers().get().unwrap_or(&ContentLength(0))),
        );

    let mut child = cmd
        .spawn_async_with_handle(&handle.new_tokio_handle())
        .expect("can't spawn CGI command");
    /* Box::new(req.body().concat2().and_then(move |body| { */
    /*     if !child.stdin().take().unwrap().write_all(&body).is_ok() { */
    /*         debug!("write_all(body) failed"); */
    /*     } */
    let r = req.body().concat2().and_then(move |body| {
        /* if !child.stdin().take().unwrap().poll_write(&body).is_ok() { */
        /*     debug!("write_all(body) failed"); */
        /* } */
        tokio_io::io::write_all(child.stdin().take().unwrap(), body)
            .and_then(move |wa| {
                child
                    .wait_with_output()
                    .map(build_response)
                    .map_err(|e| e.into())
            })
            .map_err(|e| e.into())
    });

    Box::new(r)
}

fn build_response(command_result: std::process::Output) -> Response {
    trace_begin!("build_response");
    let mut stdout = io::BufReader::new(command_result.stdout.as_slice());
    let mut stderr = io::BufReader::new(command_result.stderr.as_slice());

    let mut response = Response::new();

    let mut headers = vec![];
    for line in stdout.by_ref().lines() {
        println!("STDOUT line: {:?}", line);
        if line.as_ref().unwrap().is_empty() {
            break;
        }
        let l: Vec<&str> = line.as_ref().unwrap().as_str().splitn(2, ": ").collect();
        for x in &l {
            headers.push(x.to_string());
        }
        if l[0] == "Status" {
            response.set_status(hyper::StatusCode::Unregistered(
                u16::from_str(l[1].split(" ").next().unwrap()).unwrap(),
            ));
        } else {
            response
                .headers_mut()
                .set_raw(l[0].to_string(), l[1].to_string());
        }
    }

    let mut data = vec![];
    stdout
        .read_to_end(&mut data)
        .expect("can't read command output");

    let mut stderrdata = vec![];
    stderr
        .read_to_end(&mut stderrdata)
        .expect("can't read command output");

    /* let out = String::from_utf8_lossy(&data).to_string(); */
    let err = String::from_utf8_lossy(&stderrdata);

    /* println!("build_response out {:?}", &out); */
    println!("build_response err {:?}", &err);

    trace_end!(
        "build_response",
        /* "stdout": out, */
        "stderr": err,
        "headers": headers
    );
    response.set_body(hyper::Chunk::from(data));

    response
}
