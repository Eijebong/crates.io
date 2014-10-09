use std::io::{MemReader, IoError};
use std::io::process::{ProcessOutput, Command};
use std::collections::HashMap;
use std::fmt::Show;
use std::str;

use serialize::{json, Encodable};
use url;

use conduit::{Request, Response, Handler};

pub use self::errors::{CargoError, CargoResult, internal, human, internal_error};
pub use self::errors::{ChainError, BoxError};
pub use self::result::{Require, Wrap};
pub use self::lazy_cell::LazyCell;
pub use self::io::LimitErrorReader;
pub use self::hasher::{HashingReader};

pub mod errors;
pub mod result;
mod lazy_cell;
mod io;
mod hasher;

pub trait RequestUtils {
    fn redirect(self, url: String) -> Response;

    fn json<'a, T: Encodable<json::Encoder<'a>, IoError>>(self, t: &T) -> Response;
    fn query(self) -> HashMap<String, String>;
    fn wants_json(self) -> bool;
    fn pagination(self, default: uint, max: uint) -> CargoResult<(i64, i64)>;
}

pub fn json_response<'a, T>(t: &T) -> Response
                            where T: Encodable<json::Encoder<'a>, IoError> {
    let s = json::encode(t);
    let json = fixup(from_str(s.as_slice()).unwrap());
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(),
                   vec!["application/json; charset=utf-8".to_string()]);
    return Response {
        status: (200, "OK"),
        headers: headers,
        body: box MemReader::new(json.to_string().into_bytes()),
    };

    fn fixup(json: json::Json) -> json::Json {
        match json {
            json::Object(object) => {
                json::Object(object.into_iter().map(|(k, v)| {
                    let k = if k.as_slice() == "krate" {
                        "crate".to_string()
                    } else {
                        k
                    };
                    (k, fixup(v))
                }).collect())
            }
            json::List(list) => {
                json::List(list.into_iter().map(fixup).collect())
            }
            j => j,
        }
    }
}


impl<'a> RequestUtils for &'a Request + 'a {
    fn json<'a, T: Encodable<json::Encoder<'a>, IoError>>(self, t: &T) -> Response {
        json_response(t)
    }

    fn query(self) -> HashMap<String, String> {
        url::form_urlencoded::parse_str(self.query_string().unwrap_or(""))
            .into_iter().collect()
    }

    fn redirect(self, url: String) -> Response {
        let mut headers = HashMap::new();
        headers.insert("Location".to_string(), vec![url.to_string()]);
        Response {
            status: (302, "Found"),
            headers: headers,
            body: box MemReader::new(Vec::new()),
        }
    }

    fn wants_json(self) -> bool {
        let content = self.headers().find("Accept").unwrap_or(Vec::new());
        content.iter().any(|s| s.contains("json"))
    }

    fn pagination(self, default: uint, max: uint) -> CargoResult<(i64, i64)> {
        let query = self.query();
        let page = query.find_equiv(&"page").map(|s| s.as_slice())
                        .and_then(from_str::<uint>).unwrap_or(1);
        let limit = query.find_equiv(&"per_page").map(|s| s.as_slice())
                         .and_then(from_str::<uint>).unwrap_or(default);
        if limit > max {
            return Err(human(format!("cannot request more than {} items", max)))
        }
        Ok((((page - 1) * limit) as i64, limit as i64))
    }
}

pub struct C(pub fn(&mut Request) -> CargoResult<Response>);

impl Handler for C {
    fn call(&self, req: &mut Request) -> Result<Response, Box<Show + 'static>> {
        let C(f) = *self;
        match f(req) {
            Ok(req) => Ok(req),
            Err(e) => {
                match e.response() {
                    Some(response) => Ok(response),
                    None => Err(box e as Box<Show>),
                }
            }
        }
    }
}

pub fn exec(cmd: &Command) -> CargoResult<ProcessOutput> {
    let output = try!(cmd.output().chain_error(|| {
        internal(format!("failed to run command `{}`", cmd))
    }));
    if !output.status.success() {
        let mut desc = String::new();
        if output.output.len() != 0 {
            desc.push_str("--- stdout\n");
            desc.push_str(str::from_utf8(output.output.as_slice()).unwrap());
        }
        if output.error.len() != 0 {
            desc.push_str("--- stderr\n");
            desc.push_str(str::from_utf8(output.error.as_slice()).unwrap());
        }
        Err(internal_error(format!("failed to run command `{}`", cmd), desc))
    } else {
        Ok(output)
    }
}
