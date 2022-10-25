macro_rules! with_request {
    () => {{
        let optional_query = warp::filters::query::raw()
            .or(warp::any().map(String::default))
            .unify();

        warp::any()
            .and(warp::method())
            .and(warp::filters::path::tail())
            .and(optional_query)
            .and(warp::header::headers_cloned())
            .and(warp::body::stream())
            .map(|method, path: warp::path::Tail, query, headers, body| {
                crate::reverse_proxy::Request::new(method, path, query, headers, body)
            })
    }};
}

pub(crate) use with_request;
