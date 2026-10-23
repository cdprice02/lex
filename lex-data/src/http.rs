use std::io::{self, Read};
use std::time::Duration;

use anyhow::Context;

use crate::retry::{Failure, with_retry};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// Bounds a stalled transfer without capping the total: a healthy multi-hundred-MB shard on a
/// slow link must be allowed to finish.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// A client for the data downloads. Built without ureq's `gzip` feature, so it never asks for or
/// transparently decodes `Content-Encoding`: the sources are `.gz` files, decoded by the caller.
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RESPONSE_TIMEOUT))
        .timeout_per_read(Some(READ_TIMEOUT))
        .build()
        .into()
}

/// GETs `url` and hands the body to `consume`, retrying the whole request and read on a transient
/// failure. `consume` runs afresh on each attempt, so it must not carry state between calls.
pub(crate) fn get<T>(
    agent: &ureq::Agent,
    url: &str,
    mut consume: impl FnMut(&mut dyn Read) -> io::Result<T>,
) -> anyhow::Result<T> {
    with_retry(
        || {
            let response = agent.get(url).call().map_err(classify)?;
            let mut body = response.into_body().into_reader();
            // a dropped connection or a truncated gzip stream surfaces here, and a repeat can clear it
            consume(&mut body)
                .map_err(|e| Failure::Transient(anyhow::Error::new(e).context("reading body")))
        },
        std::thread::sleep,
    )
    .with_context(|| format!("GET {url}"))
}

fn classify(error: ureq::Error) -> Failure {
    // ureq's error enum is non-exhaustive: any variant not named here is a fault a repeat will not
    // clear (bad URI, protocol or TLS failure), so it is deliberately left to the wildcard
    #[allow(non_exhaustive_omitted_patterns)]
    let transient = match &error {
        ureq::Error::StatusCode(code) => *code >= 500 || *code == 429,
        ureq::Error::Timeout(_)
        | ureq::Error::Io(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed => true,
        _ => false,
    };
    let error = anyhow::Error::new(error);
    if transient {
        Failure::Transient(error)
    } else {
        Failure::Permanent(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_errors_and_throttling_are_transient() {
        for code in [500, 503, 429] {
            assert!(matches!(
                classify(ureq::Error::StatusCode(code)),
                Failure::Transient(_)
            ));
        }
    }

    #[test]
    fn client_errors_are_permanent() {
        for code in [400, 403, 404] {
            assert!(matches!(
                classify(ureq::Error::StatusCode(code)),
                Failure::Permanent(_)
            ));
        }
    }

    #[test]
    fn connection_faults_are_transient() {
        for error in [ureq::Error::ConnectionFailed, ureq::Error::HostNotFound] {
            assert!(matches!(classify(error), Failure::Transient(_)));
        }
    }
}
