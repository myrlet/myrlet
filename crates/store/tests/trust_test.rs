//! Worker trust: approval, configuration apply, and the Security & Trust policies.
//!
//! Split by decision surface so each module reads on its own: Pair and approval, the
//! review-and-apply path, the policy matrix, and the Delivery lifecycle that must survive
//! both.

mod trust {
    mod apply;
    mod approval;
    mod helpers;
    mod lifecycle;
    mod permission_tier;
    mod policies;
}
