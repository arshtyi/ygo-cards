use std::time::Duration;

use reqwest::StatusCode;

pub(crate) fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

pub(crate) fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_secs(u64::from(attempt * attempt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_retryable_statuses() {
        assert!(is_retryable_status(StatusCode::REQUEST_TIMEOUT));
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn uses_quadratic_backoff() {
        assert_eq!(backoff_delay(1), Duration::from_secs(1));
        assert_eq!(backoff_delay(3), Duration::from_secs(9));
    }
}
