//! Direct request-dispatch contract tests.

use crate::net::qos::RejectReason;

use super::qos_message;

#[test]
fn qos_rejections_map_to_stable_protocol_messages() {
    assert_eq!(
        qos_message(RejectReason::ConnectionBudgetExceeded),
        "connection budget exhausted"
    );
    assert_eq!(
        qos_message(RejectReason::RequestBudgetExceeded),
        "request budget exhausted"
    );
    assert_eq!(
        qos_message(RejectReason::QueueBudgetExceeded),
        "queue budget exhausted"
    );
}
