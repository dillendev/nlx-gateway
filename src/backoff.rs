macro_rules! retry_backoff {
    ($func:expr, $err:expr) => {
        use backoff::backoff::Backoff;

        let mut backoff = backoff::ExponentialBackoff::default();

        loop {
            match $func.await {
                Ok(_) => break,
                Err(e) => match backoff.next_backoff() {
                    Some(duration) => {
                        $err(e, duration);
                        tokio::time::sleep(duration).await;
                        continue;
                    }
                    None => {}
                },
            };
        }
    };
}

pub(crate) use retry_backoff;
