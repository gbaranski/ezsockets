//! Websockets Filters

use warp_crate as warp;

use std::borrow::Cow;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use warp::filters::header;
use warp::Filter;
use warp::reject::Rejection;
use warp::reply::{Reply, Response};
use http;

pub fn ws() -> impl Filter<Extract = Ws, Error = Rejection> + Copy {
    let ws = warp::filters::ws::ws();
    todo!()
    // return ;
}

pub struct Ws {
    inner: warp::filters::ws::Ws,
}
