//! Failover tests.

use oxigdal_ha::failover::{
    FailoverConfig, NodeRole,
    detection::{FailureDetector, HealthStatus, Heartbeat},
    election::{LeaderElection, VoteRequest},
    promotion::{PromotionCandidate, PromotionStrategy, ReplicaPromotion},
};
use uuid::Uuid;

#[tokio::test]
async fn test_failure_detection() {
    let config = FailoverConfig::default();
    let detector = FailureDetector::new(config);

    assert!(detector.start().await.is_ok());

    let node_id = Uuid::new_v4();
    let heartbeat = Heartbeat {
        node_id,
        timestamp: chrono::Utc::now(),
        health_status: HealthStatus::Healthy,
        sequence: 1,
    };

    assert!(detector.record_heartbeat(heartbeat).is_ok());

    let detection = detector.get_detection(node_id).ok();
    assert!(detection.is_some());

    let detection = detection.expect("failure detection should be retrieved successfully");
    assert!(!detection.is_failed);

    assert!(detector.stop().await.is_ok());
}

#[tokio::test]
async fn test_leader_election() {
    let config = FailoverConfig::default();
    let node_id = Uuid::new_v4();
    let election = LeaderElection::new(node_id, 100, config);

    assert_eq!(election.get_role(), NodeRole::Follower);

    let candidate_id = Uuid::new_v4();
    let request = VoteRequest {
        candidate_id,
        term: 1,
        priority: 50,
        timestamp: chrono::Utc::now(),
    };

    let response = election.handle_vote_request(request).await.ok();
    assert!(response.is_some());

    let response = response.expect("vote request should return a response");
    assert!(response.granted);
    assert_eq!(response.term, 1);
}

#[tokio::test]
async fn test_replica_promotion() {
    let config = FailoverConfig::default();
    let promotion = ReplicaPromotion::new(config, PromotionStrategy::Priority);

    let candidates = vec![
        PromotionCandidate {
            node_id: Uuid::new_v4(),
            name: "node1".to_string(),
            priority: 100,
            lag_ms: Some(10),
            load: 0.5,
            health_score: 1.0,
        },
        PromotionCandidate {
            node_id: Uuid::new_v4(),
            name: "node2".to_string(),
            priority: 200,
            lag_ms: Some(20),
            load: 0.6,
            health_score: 1.0,
        },
    ];

    let selected = promotion.select_candidate(candidates).await.ok();
    assert!(selected.is_some());

    let selected = selected.expect("promotion candidate should be selected");
    assert_eq!(selected.priority, 200);

    let result = promotion.promote_replica(selected).await.ok();
    assert!(result.is_some());
}
